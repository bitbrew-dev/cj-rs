use std::ffi::OsString;
use std::path::{Component, PathBuf};
use std::process::{Command, Output};

use crate::cli::ResolverOverride;
use crate::config::{Config, DefaultResolver, MountSpec};
use crate::mounts::{self, DiscoveryContext};
use crate::path_bytes;

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
        return crate::config::home_dir().ok_or("home directory is not set".into());
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
            if let Some(path) = config.aliases.get(text) {
                return resolve_configured_directory("alias", text, path);
            }
            if let Some(mount) = config.mounts.get(text) {
                return resolve_mount(text, mount);
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

fn resolve_mount(name: &str, mount: &MountSpec) -> Result<PathBuf, String> {
    if let Some(path) = mount.path.as_deref() {
        return resolve_configured_directory("mount", name, path);
    }
    let provider = mount
        .provider
        .ok_or_else(|| format!("mount {name:?} has no path or provider"))?;
    let context = DiscoveryContext::from_process()?;
    mounts::resolve_provider(provider, mount.account, &context)
        .map_err(|error| format!("mount {name:?}: {error}"))
}

fn resolve_configured_directory(
    kind: &str,
    name: &str,
    path: &std::path::Path,
) -> Result<PathBuf, String> {
    let path = expand_config_path(path).map_err(|error| format!("{kind} {name:?}: {error}"))?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("{kind} {name:?} is not reachable: {path:?}"))
    }
}

fn expand_config_path(path: &std::path::Path) -> Result<PathBuf, String> {
    let value = path.to_str().ok_or("configured path must be valid UTF-8")?;
    let Some(suffix) = crate::config::home_relative_suffix(value) else {
        return Ok(path.into());
    };
    let home = crate::config::home_dir().ok_or("cannot expand ~ because home is not set")?;
    Ok(if suffix.is_empty() {
        home
    } else {
        home.join(suffix)
    })
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
    let path = path_bytes::without_line_ending(&output.stdout);
    if path.is_empty() {
        return Err(ZoxideFailure::Query(
            "zoxide did not return a destination".into(),
        ));
    }
    path_bytes::from_bytes(path, "zoxide returned a non-UTF-8 path").map_err(ZoxideFailure::Query)
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
        .ok_or("no remembered downward route")?;
    let route_components = route.components().collect::<Vec<_>>();
    let cwd_components = navigation.cwd.components().collect::<Vec<_>>();
    if route_components.len() < cwd_components.len()
        || !route_components
            .iter()
            .zip(&cwd_components)
            .all(|(left, right)| component_eq(*left, *right))
    {
        return Err("remembered downward route is not below the current directory".into());
    }
    let components = route_components[cwd_components.len()..]
        .iter()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(*value),
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

fn component_eq(left: Component<'_>, right: Component<'_>) -> bool {
    if cfg!(windows) {
        left.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
    } else {
        left == right
    }
}

pub fn main_worktree() -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err("current directory is not in a Git repository".into());
    }
    let path = output
        .stdout
        .split(|byte| *byte == 0)
        .next()
        .and_then(|field| field.strip_prefix(b"worktree "))
        .ok_or_else(|| "git did not return a main worktree".to_owned())?;
    path_bytes::from_bytes(path, "git returned a non-UTF-8 main worktree path")
}

fn git_output<const N: usize>(args: [&str; N]) -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err("current directory is not in a Git repository".into());
    }
    let path = path_bytes::without_line_ending(&output.stdout);
    if path.is_empty() {
        return Err("git did not return a directory".into());
    }
    path_bytes::from_bytes(path, "git returned a non-UTF-8 directory")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    fn configured_destinations_follow_precedence_and_resolver_modes() {
        let root =
            std::env::temp_dir().join(format!("cj-resolver-destinations-{}", std::process::id()));
        let alias = root.join("alias destination");
        let mount = root.join("mount destination");
        fs::create_dir_all(&alias).unwrap();
        fs::create_dir_all(&mount).unwrap();

        let mut config = Config::default();
        config.aliases.insert("top".into(), alias.clone());
        config.mounts.insert(
            "origin".into(),
            MountSpec {
                path: Some(mount.clone()),
                provider: None,
                account: None,
            },
        );
        config.aliases.insert("^".into(), alias.clone());
        let navigation = NavigationContext {
            cwd: root.clone(),
            down_route: None,
        };

        assert_eq!(
            resolve(
                &["top".into()],
                ResolverOverride::NoZoxide,
                &config,
                &navigation,
            ),
            Ok(alias.clone())
        );
        assert_eq!(
            resolve(
                &["origin".into()],
                ResolverOverride::NoZoxide,
                &config,
                &navigation,
            ),
            Ok(mount)
        );
        assert_eq!(
            resolve(
                &["^".into()],
                ResolverOverride::NoZoxide,
                &config,
                &navigation,
            ),
            Ok("..".into())
        );
        assert_eq!(
            resolve(&["top".into()], ResolverOverride::Raw, &config, &navigation,),
            Ok("top".into())
        );

        config.aliases.insert("missing".into(), root.join("absent"));
        let error = resolve(
            &["missing".into()],
            ResolverOverride::Configured,
            &config,
            &navigation,
        )
        .unwrap_err();
        assert!(error.starts_with("alias \"missing\" is not reachable:"));

        fs::remove_dir_all(root).unwrap();
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
