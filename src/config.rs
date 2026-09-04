use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub behavior: Behavior,
    pub programs: Programs,
    #[serde(rename = "key-bindings")]
    pub key_bindings: KeyBindings,
    pub keywords: Keywords,
    pub tickers: Tickers,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Behavior {
    pub default: DefaultResolver,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DefaultResolver {
    Zoxide,
    Builtin,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Programs {
    pub zoxide: PathBuf,
    pub fzf: PathBuf,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct KeyBindings {
    pub macos: KeyBinding,
    pub linux: KeyBinding,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum KeyBinding {
    CtrlO,
    AltO,
    None,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Keywords {
    pub top: Vec<String>,
    #[serde(rename = "main-worktree")]
    pub main_worktree: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Tickers {
    pub navigate_up: String,
    pub navigate_down: String,
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

    fn validate(&self) -> Result<(), String> {
        validate_ticker("tickers.navigate_up", &self.tickers.navigate_up)?;
        validate_ticker("tickers.navigate_down", &self.tickers.navigate_down)?;
        if self.tickers.navigate_up == self.tickers.navigate_down {
            return Err("tickers.navigate_up and tickers.navigate_down must differ".into());
        }
        Ok(())
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

fn default_config_path() -> Option<PathBuf> {
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
}
