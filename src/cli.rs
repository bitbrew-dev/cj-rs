use std::ffi::OsString;
use std::path::PathBuf;

pub const HELP: &str = "\
cj - jump between useful directories

Usage:
  cj [OPTIONS] [TARGET]...
  cj [OPTIONS] --worktree [--format table|json] [--relative]
  cj [OPTIONS] init <bash|zsh> [--setup-key-binding]

Options:
  -C, --config <PATH>       Use a different config.toml
  -r, --raw                 Treat the target as a literal directory
  -z, --zoxide             Force zoxide resolution
  -Z, --no-zoxide          Disable zoxide while retaining cj shortcuts
  -w, --worktree           List this repository's worktrees
  -f, --format <FORMAT>    Worktree format: table or json [default: table]
  -R, --relative           Render worktree paths relative to the current directory
      --pick-worktree      Select a worktree with fzf
      --setup-key-binding  Include the OS-specific fzf binding in shell setup
  -h, --help               Print help
  -V, --version            Print version";

#[derive(Debug, PartialEq)]
pub struct Cli {
    pub config_path: Option<PathBuf>,
    pub command: Command,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Help,
    Version,
    Resolve {
        targets: Vec<OsString>,
        resolver: ResolverOverride,
    },
    Worktrees {
        format: OutputFormat,
        relative: bool,
        pick: bool,
    },
    Init {
        shell: Shell,
        setup_key_binding: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResolverOverride {
    Configured,
    Zoxide,
    NoZoxide,
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutputFormat {
    Table,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shell {
    Bash,
    Zsh,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter().peekable();
        let mut config_path = None;
        let mut resolver = ResolverOverride::Configured;
        let mut worktree = false;
        let mut format = None;
        let mut relative = false;
        let mut pick = false;
        let mut setup_key_binding = false;
        let mut targets = Vec::new();
        let mut options = true;
        let mut force_resolve = false;

        while let Some(argument) = args.next() {
            let text = argument.to_str();
            if options && text == Some("--") {
                options = false;
                force_resolve = true;
            } else if options && matches!(text, Some("-h" | "--help")) {
                return Ok(Self {
                    config_path,
                    command: Command::Help,
                });
            } else if options && matches!(text, Some("-V" | "--version")) {
                return Ok(Self {
                    config_path,
                    command: Command::Version,
                });
            } else if options && matches!(text, Some("-C" | "--config")) {
                let path = args.next().ok_or("--config requires a path")?;
                config_path = Some(PathBuf::from(path));
            } else if options && matches!(text, Some("-z" | "--zoxide")) {
                set_resolver(&mut resolver, ResolverOverride::Zoxide)?;
            } else if options && matches!(text, Some("-Z" | "--no-zoxide")) {
                set_resolver(&mut resolver, ResolverOverride::NoZoxide)?;
            } else if options && matches!(text, Some("-r" | "--raw")) {
                set_resolver(&mut resolver, ResolverOverride::Raw)?;
            } else if options && matches!(text, Some("-w" | "--worktree")) {
                worktree = true;
            } else if options && matches!(text, Some("-f" | "--format")) {
                let value = args.next().ok_or("--format requires table or json")?;
                format = Some(parse_format(&value)?);
            } else if options && matches!(text, Some("-R" | "--relative")) {
                relative = true;
            } else if options && text == Some("--pick-worktree") {
                worktree = true;
                pick = true;
            } else if options && text == Some("--setup-key-binding") {
                setup_key_binding = true;
            } else if options && text.is_some_and(|value| value.starts_with('-')) {
                return Err(format!("unknown option: {}", argument.to_string_lossy()));
            } else {
                targets.push(argument);
            }
        }

        let command = if !force_resolve
            && resolver == ResolverOverride::Configured
            && targets.first().and_then(|arg| arg.to_str()) == Some("init")
        {
            parse_init(
                &targets,
                setup_key_binding,
                worktree,
                format,
                relative,
                pick,
                resolver,
            )?
        } else if worktree {
            if !targets.is_empty() {
                return Err("--worktree does not accept a target".into());
            }
            if resolver != ResolverOverride::Configured || setup_key_binding {
                return Err("resolver and setup flags cannot be used with --worktree".into());
            }
            if pick && (format.is_some() || relative) {
                return Err("--format and --relative cannot be used with --pick-worktree".into());
            }
            Command::Worktrees {
                format: format.unwrap_or(OutputFormat::Table),
                relative,
                pick,
            }
        } else {
            if format.is_some() || relative || pick || setup_key_binding {
                return Err(
                    "--format, --relative, and setup flags require their matching mode".into(),
                );
            }
            Command::Resolve { targets, resolver }
        };

        Ok(Self {
            config_path,
            command,
        })
    }
}

fn set_resolver(current: &mut ResolverOverride, requested: ResolverOverride) -> Result<(), String> {
    if *current != ResolverOverride::Configured && *current != requested {
        return Err("--raw, --zoxide, and --no-zoxide cannot be used together".into());
    }
    *current = requested;
    Ok(())
}

fn parse_format(value: &OsString) -> Result<OutputFormat, String> {
    match value.to_str() {
        Some("table") => Ok(OutputFormat::Table),
        Some("json") => Ok(OutputFormat::Json),
        _ => Err("--format must be table or json".into()),
    }
}

fn parse_init(
    targets: &[OsString],
    setup_key_binding: bool,
    worktree: bool,
    format: Option<OutputFormat>,
    relative: bool,
    pick: bool,
    resolver: ResolverOverride,
) -> Result<Command, String> {
    if worktree || format.is_some() || relative || pick || resolver != ResolverOverride::Configured
    {
        return Err("worktree and resolver flags cannot be used with init".into());
    }
    if targets.len() != 2 {
        return Err("usage: cj init <bash|zsh> [--setup-key-binding]".into());
    }
    let shell = match targets[1].to_str() {
        Some("bash") => Shell::Bash,
        Some("zsh") => Shell::Zsh,
        _ => return Err("supported shells: bash, zsh".into()),
    };
    Ok(Command::Init {
        shell,
        setup_key_binding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_resolver_and_config_flags() {
        assert_eq!(
            Cli::parse(["-C", "other.toml", "-z", "project"].map(Into::into)),
            Ok(Cli {
                config_path: Some("other.toml".into()),
                command: Command::Resolve {
                    targets: vec!["project".into()],
                    resolver: ResolverOverride::Zoxide,
                },
            })
        );
    }

    #[test]
    fn parses_worktree_json_relative() {
        assert_eq!(
            Cli::parse(["-w", "-f", "json", "-R"].map(Into::into)),
            Ok(Cli {
                config_path: None,
                command: Command::Worktrees {
                    format: OutputFormat::Json,
                    relative: true,
                    pick: false,
                },
            })
        );
    }

    #[test]
    fn rejects_conflicting_resolvers() {
        assert_eq!(
            Cli::parse(["-z", "-Z", "project"].map(Into::into)),
            Err("--raw, --zoxide, and --no-zoxide cannot be used together".into())
        );
        assert!(Cli::parse(["-r", "-z", "project"].map(Into::into)).is_err());
        assert!(Cli::parse(["-r", "-Z", "project"].map(Into::into)).is_err());
    }

    #[test]
    fn distinguishes_raw_no_zoxide_and_relative() {
        assert!(matches!(
            Cli::parse(["-r", "top"].map(Into::into)).unwrap().command,
            Command::Resolve {
                resolver: ResolverOverride::Raw,
                ..
            }
        ));
        assert!(matches!(
            Cli::parse(["-Z", "top"].map(Into::into)).unwrap().command,
            Command::Resolve {
                resolver: ResolverOverride::NoZoxide,
                ..
            }
        ));
        assert!(matches!(
            Cli::parse(["-w", "-R"].map(Into::into)).unwrap().command,
            Command::Worktrees { relative: true, .. }
        ));
    }

    #[test]
    fn double_dash_allows_dash_prefixed_target() {
        let parsed = Cli::parse(["--", "-project"].map(Into::into)).unwrap();
        assert!(
            matches!(parsed.command, Command::Resolve { targets, .. } if targets == ["-project"])
        );
    }

    #[test]
    fn explicit_resolution_allows_init_as_a_target() {
        assert!(matches!(
            Cli::parse(["-z", "init"].map(Into::into)).unwrap().command,
            Command::Resolve { targets, resolver: ResolverOverride::Zoxide }
                if targets == ["init"]
        ));
        assert!(matches!(
            Cli::parse(["--", "init"].map(Into::into)).unwrap().command,
            Command::Resolve { targets, .. } if targets == ["init"]
        ));
    }

    #[test]
    fn picker_rejects_ignored_output_flags() {
        assert!(Cli::parse(["--pick-worktree", "-R"].map(Into::into)).is_err());
        assert!(Cli::parse(["--pick-worktree", "-f", "json"].map(Into::into)).is_err());
    }
}
