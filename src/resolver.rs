use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output};

use crate::cli::ResolverOverride;
use crate::config::{Config, DefaultResolver};

pub fn resolve(
    targets: &[OsString],
    resolver: ResolverOverride,
    config: &Config,
) -> Result<PathBuf, String> {
    if targets.is_empty() {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is not set".into());
    }

    if resolver == ResolverOverride::Builtin {
        return literal_target(targets);
    }
    if resolver == ResolverOverride::Zoxide {
        return query_zoxide(targets, config).map_err(zoxide_error);
    }

    if targets.len() == 1 {
        let target = PathBuf::from(&targets[0]);
        if target.is_dir() {
            return Ok(target);
        }

        if let Some(text) = targets[0].to_str() {
            if config.keywords.top.iter().any(|keyword| keyword == text) {
                return git_output(["rev-parse", "--show-toplevel"]);
            }
            if config
                .keywords
                .main_worktree
                .iter()
                .any(|keyword| keyword == text)
            {
                return main_worktree();
            }
            if let Some(path) = parent_ticker(text, &config.keywords.tickers) {
                return Ok(path);
            }
        }
    }

    match config.behavior.default {
        DefaultResolver::Zoxide => {
            query_zoxide(targets, config).or_else(|_| literal_target(targets))
        }
        DefaultResolver::Builtin => literal_target(targets),
    }
}

fn literal_target(targets: &[OsString]) -> Result<PathBuf, String> {
    match targets {
        [target] => Ok(PathBuf::from(target)),
        _ => Err("builtin cd accepts exactly one directory".into()),
    }
}

fn query_zoxide(targets: &[OsString], config: &Config) -> Result<PathBuf, ZoxideFailure> {
    let cwd = std::env::current_dir().map_err(|error| ZoxideFailure::Query(error.to_string()))?;
    let output = Command::new(&config.programs.zoxide)
        .arg("query")
        .arg("--exclude")
        .arg(cwd)
        .arg("--")
        .args(targets)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ZoxideFailure::Missing(config.programs.zoxide.clone())
            } else {
                ZoxideFailure::Query(error.to_string())
            }
        })?;
    parse_zoxide_output(output)
}

fn parse_zoxide_output(output: Output) -> Result<PathBuf, ZoxideFailure> {
    if !output.status.success() {
        return Err(ZoxideFailure::Query(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| ZoxideFailure::Query("zoxide returned a non-UTF-8 path".into()))?;
    let mut lines = stdout.lines();
    let path = lines
        .next()
        .filter(|line| !line.is_empty())
        .ok_or_else(|| ZoxideFailure::Query("zoxide did not return a destination".into()))?;
    if lines.next().is_some() {
        return Err(ZoxideFailure::Query(
            "zoxide returned more than one destination".into(),
        ));
    }
    Ok(path.into())
}

fn zoxide_error(error: ZoxideFailure) -> String {
    match error {
        ZoxideFailure::Missing(path) => format!("zoxide executable not found: {}", path.display()),
        ZoxideFailure::Query(message) if message.is_empty() => "zoxide query failed".into(),
        ZoxideFailure::Query(message) => format!("zoxide query failed: {message}"),
    }
}

#[derive(Debug)]
enum ZoxideFailure {
    Missing(PathBuf),
    Query(String),
}

fn parent_ticker(target: &str, tickers: &[String]) -> Option<PathBuf> {
    tickers.iter().find_map(|ticker| {
        if ticker.is_empty() {
            return None;
        }
        let mut rest = target;
        let mut path = PathBuf::new();
        let mut count = 0;
        while let Some(next) = rest.strip_prefix(ticker) {
            path.push("..");
            rest = next;
            count += 1;
        }
        (count > 0 && rest.is_empty()).then_some(path)
    })
}

fn main_worktree() -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err("current directory is not in a Git repository".into());
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .next()
        .and_then(|field| field.strip_prefix(b"worktree "))
        .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
        .ok_or("git did not return a main worktree".into())
}

fn git_output<const N: usize>(args: [&str; N]) -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err("current directory is not in a Git repository".into());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!path.is_empty())
        .then(|| PathBuf::from(path))
        .ok_or("git did not return a directory".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn repeated_ticker_becomes_parent_path() {
        assert_eq!(parent_ticker("^^^", &["^".into()]), Some("../../..".into()));
        assert_eq!(parent_ticker("^x", &["^".into()]), None);
    }

    #[test]
    fn builtin_override_preserves_literal_path() {
        let config = Config::default();
        assert_eq!(
            resolve(&["top".into()], ResolverOverride::Builtin, &config),
            Ok("top".into())
        );
    }

    #[test]
    fn forced_missing_zoxide_is_an_error() {
        let mut config = Config::default();
        config.programs.zoxide = "/definitely/missing/zoxide".into();
        assert!(
            resolve(&["project".into()], ResolverOverride::Zoxide, &config)
                .unwrap_err()
                .contains("not found")
        );
    }

    #[test]
    fn configured_missing_zoxide_falls_back_to_literal() {
        let mut config = Config::default();
        config.programs.zoxide = "/definitely/missing/zoxide".into();
        assert_eq!(
            resolve(&["project".into()], ResolverOverride::Configured, &config),
            Ok(Path::new("project").into())
        );
    }
}
