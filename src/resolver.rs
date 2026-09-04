use std::ffi::OsString;
use std::path::{Component, PathBuf};
use std::process::{Command, Output};

use crate::cli::ResolverOverride;
use crate::config::{Config, DefaultResolver};

pub fn resolve(
    targets: &[OsString],
    resolver: ResolverOverride,
    config: &Config,
    navigation: &NavigationContext,
) -> Result<PathBuf, String> {
    if resolver == ResolverOverride::Raw {
        return literal_target(targets);
    }
    if targets.is_empty() {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is not set".into());
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
            if let Some(count) = repeated_count(text, &config.tickers.navigate_up) {
                return Ok(parent_path(count));
            }
            if let Some(count) = repeated_count(text, &config.tickers.navigate_down) {
                return navigate_down(count, navigation);
            }
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
        }
    }

    if resolver == ResolverOverride::NoZoxide {
        return literal_target(targets);
    }

    match config.behavior.default {
        DefaultResolver::Zoxide => {
            query_zoxide(targets, config).or_else(|_| literal_target(targets))
        }
        DefaultResolver::Builtin => literal_target(targets),
    }
}

pub struct NavigationContext {
    cwd: PathBuf,
    down_route: Option<PathBuf>,
}

impl NavigationContext {
    pub fn from_process() -> Result<Self, String> {
        Ok(Self {
            cwd: std::env::current_dir()
                .map_err(|error| format!("cannot read current directory: {error}"))?,
            down_route: std::env::var_os("CJ_INTERNAL_DOWN_ROUTE")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
        })
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

fn repeated_count(target: &str, ticker: &str) -> Option<usize> {
    let ticker = ticker.chars().next()?;
    let mut characters = target.chars();
    let first = characters.next()?;
    (first == ticker && characters.all(|character| character == ticker))
        .then(|| target.chars().count())
}

fn parent_path(count: usize) -> PathBuf {
    std::iter::repeat_n("..", count).collect()
}

fn navigate_down(count: usize, navigation: &NavigationContext) -> Result<PathBuf, String> {
    let route = navigation
        .down_route
        .as_deref()
        .ok_or("no remembered downward route; initialize cj shell integration")?;
    let remaining = route
        .strip_prefix(&navigation.cwd)
        .map_err(|_| "remembered downward route is not below the current directory")?;
    let components = remaining
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.is_empty() {
        return Err("already at the end of the remembered downward route".into());
    }
    if count > components.len() {
        return Err(format!(
            "cannot navigate down {count} levels; only {} remembered",
            components.len()
        ));
    }
    Ok(navigation
        .cwd
        .join(components[..count].iter().collect::<PathBuf>()))
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
        assert_eq!(repeated_count("^^^", "^"), Some(3));
        assert_eq!(repeated_count("^x", "^"), None);
        assert_eq!(parent_path(3), PathBuf::from("../../.."));
    }

    #[test]
    fn raw_override_requires_one_literal_path() {
        let config = Config::default();
        let navigation = NavigationContext {
            cwd: "/repo".into(),
            down_route: None,
        };
        assert_eq!(
            resolve(&["top".into()], ResolverOverride::Raw, &config, &navigation,),
            Ok("top".into())
        );
        assert!(resolve(&[], ResolverOverride::Raw, &config, &navigation).is_err());
    }

    #[test]
    fn forced_missing_zoxide_is_an_error() {
        let mut config = Config::default();
        config.programs.zoxide = "/definitely/missing/zoxide".into();
        assert!(
            resolve(
                &["project".into()],
                ResolverOverride::Zoxide,
                &config,
                &NavigationContext {
                    cwd: "/repo".into(),
                    down_route: None,
                },
            )
            .unwrap_err()
            .contains("not found")
        );
    }

    #[test]
    fn configured_missing_zoxide_falls_back_to_literal() {
        let mut config = Config::default();
        config.programs.zoxide = "/definitely/missing/zoxide".into();
        assert_eq!(
            resolve(
                &["project".into()],
                ResolverOverride::Configured,
                &config,
                &NavigationContext {
                    cwd: "/repo".into(),
                    down_route: None,
                },
            ),
            Ok(Path::new("project").into())
        );
    }

    #[test]
    fn navigates_toward_a_remembered_descendant() {
        let navigation = NavigationContext {
            cwd: "/repo/a".into(),
            down_route: Some("/repo/a/b/c".into()),
        };
        assert_eq!(navigate_down(1, &navigation), Ok("/repo/a/b".into()));
        assert_eq!(navigate_down(2, &navigation), Ok("/repo/a/b/c".into()));
        assert!(navigate_down(3, &navigation).is_err());
    }
}
