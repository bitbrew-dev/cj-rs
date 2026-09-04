use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub behavior: Behavior,
    pub programs: Programs,
    #[serde(rename = "key-bindings")]
    pub key_bindings: KeyBindings,
    pub keywords: Keywords,
    pub tickers: Tickers,
    pub aliases: BTreeMap<String, PathBuf>,
    pub mounts: BTreeMap<String, MountSpec>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Behavior {
    pub default: DefaultResolver,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DefaultResolver {
    Zoxide,
    Builtin,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Programs {
    pub zoxide: PathBuf,
    pub fzf: PathBuf,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct KeyBindings {
    pub macos: KeyBinding,
    pub linux: KeyBinding,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum KeyBinding {
    CtrlO,
    AltO,
    None,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Keywords {
    pub top: Vec<String>,
    #[serde(rename = "main-worktree")]
    pub main_worktree: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Tickers {
    pub navigate_up: String,
    pub navigate_down: String,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MountSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<MountProvider>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum MountProvider {
    Icloud,
    GoogleDrive,
}

impl Config {
    pub fn load(explicit_path: Option<&Path>) -> Result<Self, String> {
        let path = explicit_path
            .map(PathBuf::from)
            .or_else(default_config_path);
        let Some(path) = path else {
            return Ok(Self::default());
        };

        match fs::read_to_string(&path) {
            Ok(contents) => {
                let config: Self = toml::from_str(&contents)
                    .map_err(|error| format!("invalid config {}: {error}", path.display()))?;
                config
                    .validate()
                    .map_err(|error| format!("invalid config {}: {error}", path.display()))?;
                Ok(config)
            }
            Err(error)
                if explicit_path.is_none() && error.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(Self::default())
            }
            Err(error) => Err(format!("cannot read config {}: {error}", path.display())),
        }
    }

    pub fn key_binding(&self) -> KeyBinding {
        match env::consts::OS {
            "macos" => self.key_bindings.macos,
            "linux" => self.key_bindings.linux,
            _ => KeyBinding::None,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        validate_ticker("tickers.navigate_up", &self.tickers.navigate_up)?;
        validate_ticker("tickers.navigate_down", &self.tickers.navigate_down)?;
        if self.tickers.navigate_up == self.tickers.navigate_down {
            return Err("tickers.navigate_up and tickers.navigate_down must differ".into());
        }
        for keyword in &self.keywords.top {
            validate_name("top keyword", keyword)?;
        }
        for keyword in &self.keywords.main_worktree {
            validate_name("main-worktree keyword", keyword)?;
        }
        for (name, path) in &self.aliases {
            validate_name("alias", name)?;
            validate_config_path(&format!("aliases.{name}"), path)?;
        }
        for (name, mount) in &self.mounts {
            validate_name("mount", name)?;
            if self.aliases.contains_key(name) {
                return Err(format!(
                    "name {name:?} is used by both an alias and a mount"
                ));
            }
            mount.validate(name)?;
        }
        Ok(())
    }
}

impl MountSpec {
    fn validate(&self, name: &str) -> Result<(), String> {
        match (&self.path, self.provider) {
            (Some(path), None) => validate_config_path(&format!("mounts.{name}.path"), path),
            (None, Some(_)) => Ok(()),
            (None, None) => Err(format!(
                "mounts.{name} must specify exactly one of path or provider"
            )),
            (Some(_), Some(_)) => Err(format!(
                "mounts.{name} cannot specify both path and provider"
            )),
        }
    }
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            default: DefaultResolver::Zoxide,
        }
    }
}

impl Default for Programs {
    fn default() -> Self {
        Self {
            zoxide: "zoxide".into(),
            fzf: "fzf".into(),
        }
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            macos: KeyBinding::CtrlO,
            linux: KeyBinding::AltO,
        }
    }
}

impl Default for Keywords {
    fn default() -> Self {
        Self {
            top: vec!["top".into()],
            main_worktree: vec!["origin".into(), "og".into()],
        }
    }
}

impl Default for Tickers {
    fn default() -> Self {
        Self {
            navigate_up: "^".into(),
            navigate_down: "v".into(),
        }
    }
}

const ALLOWED_TICKERS: &[char] = &['^', 'v', 'u', 'd', 'j', 'k'];

fn validate_name(kind: &str, name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err(format!("{kind} name must not be empty"));
    }
    if name.contains(['\0', '\n', '\r']) {
        return Err(format!("{kind} name {name:?} must fit on one line"));
    }
    Ok(())
}

fn validate_config_path(name: &str, path: &Path) -> Result<(), String> {
    let value = path
        .to_str()
        .ok_or_else(|| format!("{name} must be valid UTF-8"))?;
    if value.contains(['\0', '\n', '\r']) {
        return Err(format!("{name} must fit on one line"));
    }
    if path.is_absolute() || value == "~" || value.starts_with("~/") {
        Ok(())
    } else {
        Err(format!("{name} must be absolute or start with ~/"))
    }
}

fn validate_ticker(name: &str, ticker: &str) -> Result<(), String> {
    let mut characters = ticker.chars();
    let Some(character) = characters.next() else {
        return Err(format!("{name} must be exactly one character"));
    };
    if characters.next().is_some() {
        return Err(format!("{name} must be exactly one character"));
    }
    if !ALLOWED_TICKERS.contains(&character) {
        return Err(format!(
            "{name} {ticker:?} is not allowed; allowed values: ^, v, u, d, j, k"
        ));
    }
    Ok(())
}

pub fn default_config_path() -> Option<PathBuf> {
    config_path_from(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"))
}

fn config_path_from(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    xdg.filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|path| !path.is_empty())
                .map(|path| PathBuf::from(path).join(".config"))
        })
        .map(|path| path.join("cj/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_inherits_defaults() {
        let config: Config = toml::from_str(
            r#"
                [behavior]
                default = "builtin"

                [programs]
                zoxide = "/opt/bin/zoxide"
            "#,
        )
        .unwrap();

        assert_eq!(config.behavior.default, DefaultResolver::Builtin);
        assert_eq!(config.programs.zoxide, Path::new("/opt/bin/zoxide"));
        assert_eq!(config.programs.fzf, Path::new("fzf"));
        assert_eq!(config.key_bindings.macos, KeyBinding::CtrlO);
        assert_eq!(config.tickers, Tickers::default());
    }

    #[test]
    fn computes_xdg_and_home_paths() {
        assert_eq!(
            config_path_from(Some("/xdg".into()), Some("/home/me".into())),
            Some("/xdg/cj/config.toml".into())
        );
        assert_eq!(
            config_path_from(None, Some("/home/me".into())),
            Some("/home/me/.config/cj/config.toml".into())
        );
        assert_eq!(
            config_path_from(Some("".into()), Some("/home/me".into())),
            Some("/home/me/.config/cj/config.toml".into())
        );
        assert_eq!(config_path_from(None, Some("".into())), None);
    }

    #[test]
    fn validates_navigation_tickers() {
        for invalid in [
            "", "vv", ">", "<", "|", "&", ";", "$", "'", "\"", "*", "?", "\\", " ", "\t",
        ] {
            let mut config = Config::default();
            config.tickers.navigate_down = invalid.into();
            assert!(
                config
                    .validate()
                    .unwrap_err()
                    .contains("tickers.navigate_down")
            );
        }

        let mut config = Config::default();
        config.tickers.navigate_up = "k".into();
        config.tickers.navigate_down = "j".into();
        assert_eq!(config.validate(), Ok(()));

        config.tickers.navigate_down = "k".into();
        assert_eq!(
            config.validate(),
            Err("tickers.navigate_up and tickers.navigate_down must differ".into())
        );
    }

    #[test]
    fn validates_aliases_mounts_and_name_collisions() {
        let absolute = env::temp_dir().join("CJ Tests/it's mounted");
        let mut config = Config::default();
        config.aliases.insert("code".into(), "~/Git".into());
        config.mounts.insert(
            "external-ssd".into(),
            MountSpec {
                path: Some(absolute),
                provider: None,
            },
        );
        config.mounts.insert(
            "google-work".into(),
            MountSpec {
                path: None,
                provider: Some(MountProvider::GoogleDrive),
            },
        );
        assert_eq!(config.validate(), Ok(()));

        config.mounts.insert(
            "code".into(),
            MountSpec {
                path: Some("/mnt/code".into()),
                provider: None,
            },
        );
        assert_eq!(
            config.validate(),
            Err("name \"code\" is used by both an alias and a mount".into())
        );
    }

    #[test]
    fn rejects_multiline_keywords_before_shell_generation() {
        let mut config = Config::default();
        config.keywords.top = vec!["line\nbreak".into()];
        assert!(config.validate().unwrap_err().contains("top keyword"));
    }

    #[test]
    fn mount_requires_exactly_one_source() {
        let mut config = Config::default();
        config.mounts.insert("empty".into(), MountSpec::default());
        assert!(config.validate().unwrap_err().contains("exactly one"));

        config.mounts.insert(
            "empty".into(),
            MountSpec {
                path: Some("/mnt/example".into()),
                provider: Some(MountProvider::Icloud),
            },
        );
        assert!(
            config
                .validate()
                .unwrap_err()
                .contains("cannot specify both")
        );
    }

    #[test]
    fn paths_must_be_absolute_tilde_based_and_line_safe() {
        for invalid in ["relative/path", "~someone/path", "/tmp/line\nbreak"] {
            let mut config = Config::default();
            config.aliases.insert("bad".into(), invalid.into());
            assert!(config.validate().is_err(), "accepted {invalid:?}");
        }
        let mut config = Config::default();
        config.aliases.insert("quote".into(), "~/it's here".into());
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn serializes_maps_deterministically_and_round_trips_providers() {
        let mut config = Config::default();
        config.aliases.insert("z-last".into(), "~/Z".into());
        config.aliases.insert("a-first".into(), "~/A".into());
        config.mounts.insert(
            "icloud".into(),
            MountSpec {
                path: None,
                provider: Some(MountProvider::Icloud),
            },
        );
        let encoded = toml::to_string_pretty(&config).unwrap();
        assert!(encoded.find("a-first").unwrap() < encoded.find("z-last").unwrap());
        assert!(encoded.contains("provider = \"icloud\""));
        let decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, config);
    }
}
