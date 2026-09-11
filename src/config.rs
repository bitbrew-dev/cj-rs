use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::{self, MapAccess, Visitor, value::MapAccessDeserializer};
use serde::{Deserialize, Deserializer, Serialize};

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
    #[serde(deserialize_with = "deserialize_macos_binding")]
    pub macos: BindingConfig,
    #[serde(deserialize_with = "deserialize_linux_binding")]
    pub linux: BindingConfig,
    #[serde(deserialize_with = "deserialize_windows_binding")]
    pub windows: BindingConfig,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct BindingConfig {
    pub key: KeyBinding,
    pub behaviors: Vec<KeyBindingBehavior>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum KeyBindingBehavior {
    Zoxide,
    Cj,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<OneDriveAccount>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum MountProvider {
    Icloud,
    GoogleDrive,
    #[serde(rename = "onedrive", alias = "one-drive")]
    OneDrive,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OneDriveAccount {
    Personal,
    Business,
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
        if self.key_binding_behaviors().is_empty() {
            return KeyBinding::None;
        }
        self.key_bindings
            .for_os(env::consts::OS)
            .map_or(KeyBinding::None, |binding| binding.key)
    }

    pub fn key_binding_behaviors(&self) -> &[KeyBindingBehavior] {
        self.key_bindings
            .for_os(env::consts::OS)
            .filter(|binding| binding.key != KeyBinding::None)
            .map_or(&[], |binding| binding.behaviors.as_slice())
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        for (os, binding) in [
            ("macos", &self.key_bindings.macos),
            ("linux", &self.key_bindings.linux),
            ("windows", &self.key_bindings.windows),
        ] {
            binding
                .validate()
                .map_err(|error| format!("key-bindings.{os}: {error}"))?;
        }
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
        match (&self.path, self.provider, self.account) {
            (Some(path), None, None) => validate_config_path(&format!("mounts.{name}.path"), path),
            (None, Some(MountProvider::OneDrive), _) => Ok(()),
            (None, Some(_), None) => Ok(()),
            (None, None, None) => Err(format!(
                "mounts.{name} must specify exactly one of path or provider"
            )),
            (Some(_), _, _) => Err(format!(
                "mounts.{name} cannot specify both path and provider or account"
            )),
            (None, None, Some(_)) => Err(format!(
                "mounts.{name}.account requires provider = \"onedrive\""
            )),
            (None, Some(_), Some(_)) => Err(format!(
                "mounts.{name}.account is supported only by provider = \"onedrive\""
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
            macos: BindingConfig::from_key(KeyBinding::CtrlO),
            linux: BindingConfig::from_key(KeyBinding::AltO),
            windows: BindingConfig::from_key(KeyBinding::CtrlO),
        }
    }
}

impl KeyBindings {
    fn for_os(&self, os: &str) -> Option<&BindingConfig> {
        match os {
            "macos" => Some(&self.macos),
            "linux" => Some(&self.linux),
            "windows" => Some(&self.windows),
            _ => None,
        }
    }
}

impl BindingConfig {
    fn from_key(key: KeyBinding) -> Self {
        Self {
            key,
            behaviors: if key == KeyBinding::None {
                vec![]
            } else {
                vec![KeyBindingBehavior::Zoxide, KeyBindingBehavior::Cj]
            },
        }
    }

    fn validate(&self) -> Result<(), String> {
        for (index, behavior) in self.behaviors.iter().enumerate() {
            if self.behaviors[..index].contains(behavior) {
                let name = match behavior {
                    KeyBindingBehavior::Zoxide => "zoxide",
                    KeyBindingBehavior::Cj => "cj",
                };
                return Err(format!(
                    "behaviors contains duplicate {name:?}; use each behavior at most once"
                ));
            }
        }
        Ok(())
    }
}

fn deserialize_macos_binding<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BindingConfig, D::Error> {
    deserialize_binding(deserializer, KeyBinding::CtrlO, "macos")
}

fn deserialize_linux_binding<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BindingConfig, D::Error> {
    deserialize_binding(deserializer, KeyBinding::AltO, "linux")
}

fn deserialize_windows_binding<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BindingConfig, D::Error> {
    deserialize_binding(deserializer, KeyBinding::CtrlO, "windows")
}

fn deserialize_binding<'de, D: Deserializer<'de>>(
    deserializer: D,
    default_key: KeyBinding,
    os: &str,
) -> Result<BindingConfig, D::Error> {
    struct BindingVisitor(KeyBinding);

    impl<'de> Visitor<'de> for BindingVisitor {
        type Value = BindingConfig;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter
                .write_str("a key string (ctrl-o, alt-o, none) or a table with key and behaviors")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            KeyBinding::deserialize(de::value::StrDeserializer::<E>::new(value))
                .map(BindingConfig::from_key)
        }

        fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Fields {
                key: Option<KeyBinding>,
                behaviors: Option<Vec<KeyBindingBehavior>>,
            }

            let fields = Fields::deserialize(MapAccessDeserializer::new(map))?;
            let mut binding = BindingConfig::from_key(fields.key.unwrap_or(self.0));
            if let Some(behaviors) = fields.behaviors {
                binding.behaviors = behaviors;
            }
            binding.validate().map_err(de::Error::custom)?;
            Ok(binding)
        }
    }

    deserializer
        .deserialize_any(BindingVisitor(default_key))
        .map_err(|error| de::Error::custom(format!("key-bindings.{os}: {error}")))
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
    if path.is_absolute() || home_relative_suffix(value).is_some() {
        Ok(())
    } else {
        Err(format!(
            "{name} must be absolute or start with ~/{}",
            if cfg!(windows) { " or ~\\" } else { "" }
        ))
    }
}

pub(crate) fn home_relative_suffix(value: &str) -> Option<&str> {
    if value == "~" {
        Some("")
    } else {
        value.strip_prefix("~/").or_else(|| {
            if cfg!(windows) {
                value.strip_prefix("~\\")
            } else {
                None
            }
        })
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
    config_path_from(
        env::var_os("XDG_CONFIG_HOME"),
        env::var_os("APPDATA"),
        home_dir(),
        cfg!(windows),
    )
}

pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .or_else(|| env::var_os("USERPROFILE").filter(|path| !path.is_empty()))
        .map(PathBuf::from)
}

fn config_path_from(
    xdg: Option<OsString>,
    appdata: Option<OsString>,
    home: Option<PathBuf>,
    windows: bool,
) -> Option<PathBuf> {
    xdg.filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            windows
                .then_some(appdata)
                .flatten()
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            home.filter(|path| !path.as_os_str().is_empty())
                .map(|path| path.join(".config"))
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
        assert_eq!(config.key_bindings, KeyBindings::default());
        assert_eq!(config.tickers, Tickers::default());
    }

    #[test]
    fn legacy_key_bindings_keep_default_behaviors_and_none_disables() {
        let config: Config = toml::from_str(
            r#"[key-bindings]
macos = "alt-o"
linux = "ctrl-o"
windows = "none"
"#,
        )
        .unwrap();
        assert_eq!(
            config.key_bindings.macos,
            BindingConfig::from_key(KeyBinding::AltO)
        );
        assert_eq!(
            config.key_bindings.linux,
            BindingConfig::from_key(KeyBinding::CtrlO)
        );
        assert_eq!(
            config.key_bindings.windows,
            BindingConfig::from_key(KeyBinding::None)
        );
        assert!(config.key_bindings.windows.behaviors.is_empty());
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn structured_key_bindings_preserve_order_and_allow_single_or_empty_behaviors() {
        for behaviors in [
            vec![KeyBindingBehavior::Zoxide, KeyBindingBehavior::Cj],
            vec![KeyBindingBehavior::Cj, KeyBindingBehavior::Zoxide],
            vec![KeyBindingBehavior::Zoxide],
            vec![KeyBindingBehavior::Cj],
            vec![],
        ] {
            let names: Vec<_> = behaviors
                .iter()
                .map(|behavior| match behavior {
                    KeyBindingBehavior::Zoxide => "\"zoxide\"",
                    KeyBindingBehavior::Cj => "\"cj\"",
                })
                .collect();
            let config: Config = toml::from_str(&format!(
                "[key-bindings]\nmacos = {{ key = \"ctrl-o\", behaviors = [{}] }}\nlinux = {{ key = \"alt-o\", behaviors = [{}] }}\nwindows = {{ key = \"ctrl-o\", behaviors = [{}] }}\n",
                names.join(", "), names.join(", "), names.join(", "),
            ))
            .unwrap();
            for os in ["macos", "linux", "windows"] {
                assert_eq!(config.key_bindings.for_os(os).unwrap().behaviors, behaviors);
            }
            assert_eq!(config.key_binding_behaviors(), behaviors);
            assert_eq!(config.validate(), Ok(()));
            if behaviors.is_empty() {
                assert_eq!(config.key_binding(), KeyBinding::None);
            }
        }
    }

    #[test]
    fn structured_binding_defaults_and_os_selection_are_deterministic() {
        let config: Config = toml::from_str(
            r#"[key-bindings]
macos = {}
linux = { behaviors = ["cj"] }
windows = { key = "alt-o" }
"#,
        )
        .unwrap();
        assert_eq!(
            config.key_bindings.for_os("macos"),
            Some(&BindingConfig::from_key(KeyBinding::CtrlO))
        );
        assert_eq!(
            config.key_bindings.for_os("linux"),
            Some(&BindingConfig {
                key: KeyBinding::AltO,
                behaviors: vec![KeyBindingBehavior::Cj],
            })
        );
        assert_eq!(
            config.key_bindings.for_os("windows"),
            Some(&BindingConfig::from_key(KeyBinding::AltO))
        );
        assert_eq!(config.key_bindings.for_os("freebsd"), None);

        let defaults: Config = toml::from_str("[key-bindings]\nlinux = {}\n").unwrap();
        assert_eq!(defaults.key_bindings, KeyBindings::default());
    }

    #[test]
    fn invalid_bindings_report_os_and_actionable_reason() {
        for os in ["macos", "linux", "windows"] {
            for (value, reason) in [
                (r#""ctrl-x""#, "expected one of `ctrl-o`, `alt-o`, `none`"),
                (
                    r#"{ key = "ctrl-x" }"#,
                    "expected one of `ctrl-o`, `alt-o`, `none`",
                ),
                (r#"{ behaviors = ["auto"] }"#, "expected `zoxide` or `cj`"),
                (r#"{ behaviors = ["cj", "cj"] }"#, "duplicate \"cj\""),
                (
                    r#"{ behaviors = ["zoxide", "zoxide"] }"#,
                    "duplicate \"zoxide\"",
                ),
                (r#"{ behavior = ["cj"] }"#, "unknown field `behavior`"),
                (r#"{ behaviors = "cj" }"#, "expected a sequence"),
                ("42", "a key string"),
            ] {
                let error = toml::from_str::<Config>(&format!("[key-bindings]\n{os} = {value}\n"))
                    .unwrap_err()
                    .to_string();
                assert!(error.contains(&format!("key-bindings.{os}")), "{error}");
                assert!(error.contains(reason), "{error}");
            }
        }
    }

    #[test]
    fn validate_rejects_duplicate_behaviors_on_any_os() {
        for os in ["macos", "linux", "windows"] {
            let mut config = Config::default();
            let binding = match os {
                "macos" => &mut config.key_bindings.macos,
                "linux" => &mut config.key_bindings.linux,
                _ => &mut config.key_bindings.windows,
            };
            binding.behaviors = vec![KeyBindingBehavior::Cj, KeyBindingBehavior::Cj];
            let error = config.validate().unwrap_err();
            assert!(error.contains(&format!("key-bindings.{os}")), "{error}");
            assert!(error.contains("use each behavior at most once"), "{error}");
        }
    }

    #[test]
    fn serializes_bindings_canonically_and_round_trips_legacy_and_structured_forms() {
        let config: Config = toml::from_str(
            r#"[key-bindings]
macos = "ctrl-o"
linux = { key = "alt-o", behaviors = ["cj", "zoxide"] }
windows = "none"
"#,
        )
        .unwrap();
        let encoded = toml::to_string_pretty(&config).unwrap();
        assert!(encoded.contains("[key-bindings.macos]\nkey = \"ctrl-o\"\nbehaviors = [\n    \"zoxide\",\n    \"cj\",\n]"), "{encoded}");
        assert!(
            encoded.contains("[key-bindings.windows]\nkey = \"none\"\nbehaviors = []"),
            "{encoded}"
        );
        let decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, config);
    }

    #[test]
    fn computes_xdg_and_home_paths() {
        assert_eq!(
            config_path_from(
                Some("/xdg".into()),
                Some("/appdata".into()),
                Some("/home/me".into()),
                false,
            ),
            Some("/xdg/cj/config.toml".into())
        );
        assert_eq!(
            config_path_from(None, None, Some("/home/me".into()), false),
            Some("/home/me/.config/cj/config.toml".into())
        );
        assert_eq!(
            config_path_from(Some("".into()), None, Some("/home/me".into()), false),
            Some("/home/me/.config/cj/config.toml".into())
        );
        assert_eq!(config_path_from(None, None, None, false), None);
        assert_eq!(
            config_path_from(
                None,
                Some("C:\\Users\\me\\AppData\\Roaming".into()),
                Some("C:\\Users\\me".into()),
                true,
            ),
            Some("C:\\Users\\me\\AppData\\Roaming/cj/config.toml".into())
        );
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
                account: None,
            },
        );
        config.mounts.insert(
            "google-work".into(),
            MountSpec {
                path: None,
                provider: Some(MountProvider::GoogleDrive),
                account: None,
            },
        );
        assert_eq!(config.validate(), Ok(()));

        config.mounts.insert(
            "code".into(),
            MountSpec {
                path: Some("/mnt/code".into()),
                provider: None,
                account: None,
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
                account: None,
            },
        );
        assert!(
            config
                .validate()
                .unwrap_err()
                .contains("cannot specify both")
        );

        let mut config = Config::default();
        config.mounts.insert(
            "work".into(),
            MountSpec {
                path: None,
                provider: Some(MountProvider::OneDrive),
                account: Some(OneDriveAccount::Business),
            },
        );
        assert_eq!(config.validate(), Ok(()));

        config.mounts.insert(
            "invalid".into(),
            MountSpec {
                path: None,
                provider: Some(MountProvider::Icloud),
                account: Some(OneDriveAccount::Personal),
            },
        );
        assert!(config.validate().unwrap_err().contains("onedrive"));
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
                account: None,
            },
        );
        let encoded = toml::to_string_pretty(&config).unwrap();
        assert!(encoded.find("a-first").unwrap() < encoded.find("z-last").unwrap());
        assert!(encoded.contains("provider = \"icloud\""));
        let decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, config);
    }

    #[test]
    fn onedrive_accepts_legacy_spelling_and_serializes_canonical_name() {
        for provider in ["onedrive", "one-drive"] {
            let config: Config = toml::from_str(&format!(
                "[mounts.work]\nprovider = \"{provider}\"\naccount = \"business\"\n"
            ))
            .unwrap();
            assert_eq!(config.validate(), Ok(()));
            assert_eq!(
                config.mounts["work"].provider,
                Some(MountProvider::OneDrive)
            );
            assert_eq!(
                config.mounts["work"].account,
                Some(OneDriveAccount::Business)
            );

            let encoded = toml::to_string_pretty(&config).unwrap();
            let document: toml::Value = toml::from_str(&encoded).unwrap();
            assert_eq!(
                document["mounts"]["work"]["provider"].as_str(),
                Some("onedrive")
            );
            let decoded: Config = toml::from_str(&encoded).unwrap();
            assert_eq!(decoded, config);
        }
    }
}
