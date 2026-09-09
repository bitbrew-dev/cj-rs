use std::ffi::OsString;
use std::path::PathBuf;

pub const HELP: &str = "\
cj - jump between useful directories

Usage:
  cj [OPTIONS] [TARGET]...
  cj [OPTIONS] --worktree [--format table|json] [--relative]
  cj [OPTIONS] --jump-worktree
  cj [OPTIONS] config init [--preamp]
  cj [OPTIONS] mounts scan [--format table|json]
  cj [OPTIONS] init <bash|zsh|nu|powershell> [--setup-key-binding] [-o PATH]
  cj [OPTIONS] completions <bash|zsh|nu|powershell> [-o PATH]

Options:
  -C, --config <PATH>       Use a different config.toml
  -v, --verbose             Show discovery details and skipped candidates
  -r, --raw                 Treat the target as a literal directory
  -z, --zoxide             Force zoxide resolution
  -Z, --no-zoxide          Disable zoxide while retaining cj shortcuts
  -w, --worktree           List this repository's worktrees
  -f, --format <FORMAT>    Output format: table or json [default: table]
  -R, --relative           Render worktree paths relative to the current directory
  -jw, --jump-worktree     Select a worktree with fzf and print its path
      --setup-key-binding  Include the OS-specific fzf binding in shell setup
  -o, --output <PATH>      Write generated shell source and create parent directories
      --preamp             Add unambiguous mounts to a new config
  -h, --help               Print help
  -V, --version            Print version";

#[derive(Debug, PartialEq)]
pub struct Cli {
    pub config_path: Option<PathBuf>,
    pub verbose: bool,
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
        jump: bool,
    },
    WorktreePaths0,
    KeyBindingZoxide {
        zoxide: PathBuf,
        fzf: PathBuf,
    },
    Init {
        shell: Shell,
        setup_key_binding: bool,
        output: Option<PathBuf>,
    },
    Completions {
        shell: Shell,
        output: Option<PathBuf>,
    },
    ConfigInit {
        preamp: bool,
    },
    MountsScan {
        format: OutputFormat,
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
    Nu,
    Pwsh,
}

struct ShellOptions {
    setup_key_binding: bool,
    output: Option<PathBuf>,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter().peekable();
        let mut config_path = None;
        let mut verbose = false;
        let mut resolver = ResolverOverride::Configured;
        let mut worktree = false;
        let mut format = None;
        let mut relative = false;
        let mut jump = false;
        let mut worktree_paths0 = false;
        let mut key_binding_zoxide = None;
        let mut setup_key_binding = false;
        let mut output = None;
        let mut preamp = false;
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
                    verbose,
                    command: Command::Help,
                });
            } else if options && matches!(text, Some("-V" | "--version")) {
                return Ok(Self {
                    config_path,
                    verbose,
                    command: Command::Version,
                });
            } else if options && matches!(text, Some("-C" | "--config")) {
                let path = args.next().ok_or("--config requires a path")?;
                config_path = Some(PathBuf::from(path));
            } else if options && matches!(text, Some("-v" | "--verbose")) {
                verbose = true;
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
            } else if options && matches!(text, Some("-jw" | "--jump-worktree")) {
                worktree = true;
                jump = true;
            } else if options && text == Some("--worktree-paths0") {
                worktree_paths0 = true;
            } else if options && text == Some("--internal-key-binding-zoxide") {
                if key_binding_zoxide.is_some() {
                    return Err("--internal-key-binding-zoxide cannot be repeated".into());
                }
                let zoxide = args
                    .next()
                    .ok_or("--internal-key-binding-zoxide requires zoxide and fzf paths")?;
                let fzf = args
                    .next()
                    .ok_or("--internal-key-binding-zoxide requires zoxide and fzf paths")?;
                key_binding_zoxide = Some((PathBuf::from(zoxide), PathBuf::from(fzf)));
            } else if options && text == Some("--setup-key-binding") {
                setup_key_binding = true;
            } else if options && matches!(text, Some("-o" | "--output")) {
                output = Some(PathBuf::from(
                    args.next().ok_or("--output requires a path")?,
                ));
            } else if options && text == Some("--preamp") {
                preamp = true;
            } else if options && text.is_some_and(|value| value.starts_with('-')) {
                return Err(format!("unknown option: {}", argument.to_string_lossy()));
            } else {
                targets.push(argument);
            }
        }

        let command = if let Some((zoxide, fzf)) = key_binding_zoxide {
            if force_resolve
                || !targets.is_empty()
                || worktree_paths0
                || worktree
                || jump
                || format.is_some()
                || relative
                || resolver != ResolverOverride::Configured
                || setup_key_binding
                || preamp
                || config_path.is_some()
                || output.is_some()
            {
                return Err(
                    "--internal-key-binding-zoxide cannot be combined with another mode".into(),
                );
            }
            Command::KeyBindingZoxide { zoxide, fzf }
        } else if worktree_paths0 {
            if force_resolve
                || !targets.is_empty()
                || worktree
                || format.is_some()
                || relative
                || resolver != ResolverOverride::Configured
                || setup_key_binding
                || preamp
            {
                return Err("--worktree-paths0 cannot be combined with another mode".into());
            }
            Command::WorktreePaths0
        } else if !force_resolve
            && resolver == ResolverOverride::Configured
            && targets.first().and_then(|arg| arg.to_str()) == Some("config")
        {
            parse_config_init(
                &targets,
                preamp,
                setup_key_binding,
                worktree,
                format,
                relative,
                jump,
            )?
        } else if !force_resolve
            && resolver == ResolverOverride::Configured
            && targets.first().and_then(|arg| arg.to_str()) == Some("mounts")
        {
            parse_mounts_scan(
                &targets,
                preamp,
                setup_key_binding,
                worktree,
                format,
                relative,
                jump,
            )?
        } else if !force_resolve
            && resolver == ResolverOverride::Configured
            && targets.first().and_then(|arg| arg.to_str()) == Some("init")
        {
            parse_init(
                &targets,
                ShellOptions {
                    setup_key_binding,
                    output: output.clone(),
                },
                worktree,
                format,
                relative,
                jump,
                preamp,
            )?
        } else if !force_resolve
            && resolver == ResolverOverride::Configured
            && targets.first().and_then(|arg| arg.to_str()) == Some("completions")
        {
            parse_completions(
                &targets,
                ShellOptions {
                    setup_key_binding,
                    output: output.clone(),
                },
                worktree,
                format,
                relative,
                jump,
                preamp,
            )?
        } else if worktree {
            if !targets.is_empty() {
                return Err("--worktree does not accept a target".into());
            }
            if resolver != ResolverOverride::Configured || setup_key_binding || preamp {
                return Err("resolver and setup flags cannot be used with --worktree".into());
            }
            if jump && (format.is_some() || relative) {
                return Err("--format and --relative cannot be used with --jump-worktree".into());
            }
            Command::Worktrees {
                format: format.unwrap_or(OutputFormat::Table),
                relative,
                jump,
            }
        } else {
            if format.is_some() || relative || jump || setup_key_binding || preamp {
                return Err(
                    "--format, --relative, and setup flags require their matching mode".into(),
                );
            }
            Command::Resolve { targets, resolver }
        };

        if output.is_some()
            && !matches!(command, Command::Init { .. } | Command::Completions { .. })
        {
            return Err("--output can only be used with init or completions".into());
        }

        Ok(Self {
            config_path,
            verbose,
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
    options: ShellOptions,
    worktree: bool,
    format: Option<OutputFormat>,
    relative: bool,
    jump: bool,
    preamp: bool,
) -> Result<Command, String> {
    if worktree || format.is_some() || relative || jump || preamp {
        return Err("worktree and resolver flags cannot be used with init".into());
    }
    if targets.len() != 2 {
        return Err(
            "usage: cj init <bash|zsh|nu|powershell> [--setup-key-binding] [-o PATH]".into(),
        );
    }
    let shell = parse_shell(&targets[1])?;
    if options.setup_key_binding && shell == Shell::Nu {
        return Err(
            "--setup-key-binding is currently supported for bash, zsh, and PowerShell".into(),
        );
    }
    Ok(Command::Init {
        shell,
        setup_key_binding: options.setup_key_binding,
        output: options.output,
    })
}

fn parse_completions(
    targets: &[OsString],
    options: ShellOptions,
    worktree: bool,
    format: Option<OutputFormat>,
    relative: bool,
    jump: bool,
    preamp: bool,
) -> Result<Command, String> {
    if options.setup_key_binding || worktree || format.is_some() || relative || jump || preamp {
        return Err(
            "worktree, format, relative, preamp, and setup flags cannot be used with completions"
                .into(),
        );
    }
    if targets.len() != 2 {
        return Err("usage: cj completions <bash|zsh|nu|powershell> [-o PATH]".into());
    }
    Ok(Command::Completions {
        shell: parse_shell(&targets[1])?,
        output: options.output,
    })
}

fn parse_shell(value: &OsString) -> Result<Shell, String> {
    match value.to_str() {
        Some("bash") => Ok(Shell::Bash),
        Some("zsh") => Ok(Shell::Zsh),
        Some("nu") => Ok(Shell::Nu),
        Some("powershell" | "pwsh") => Ok(Shell::Pwsh),
        _ => Err("supported shells: bash, zsh, nu, powershell".into()),
    }
}

fn parse_config_init(
    targets: &[OsString],
    preamp: bool,
    setup_key_binding: bool,
    worktree: bool,
    format: Option<OutputFormat>,
    relative: bool,
    jump: bool,
) -> Result<Command, String> {
    if targets.len() != 2 || targets[1].to_str() != Some("init") {
        return Err("usage: cj config init [--preamp]".into());
    }
    if setup_key_binding || worktree || format.is_some() || relative || jump {
        return Err(
            "worktree, output, and shell setup flags cannot be used with config init".into(),
        );
    }
    Ok(Command::ConfigInit { preamp })
}

fn parse_mounts_scan(
    targets: &[OsString],
    preamp: bool,
    setup_key_binding: bool,
    worktree: bool,
    format: Option<OutputFormat>,
    relative: bool,
    jump: bool,
) -> Result<Command, String> {
    if targets.len() != 2 || targets[1].to_str() != Some("scan") {
        return Err("usage: cj mounts scan [--format table|json]".into());
    }
    if preamp || setup_key_binding || worktree || relative || jump {
        return Err(
            "worktree, relative, preamp, and shell setup flags cannot be used with mounts scan"
                .into(),
        );
    }
    Ok(Command::MountsScan {
        format: format.unwrap_or(OutputFormat::Table),
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
                verbose: false,
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
                verbose: false,
                command: Command::Worktrees {
                    format: OutputFormat::Json,
                    relative: true,
                    jump: false,
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
    fn parses_exact_jump_worktree_options() {
        for option in ["-jw", "--jump-worktree"] {
            assert_eq!(
                Cli::parse([option].map(Into::into)),
                Ok(Cli {
                    config_path: None,
                    verbose: false,
                    command: Command::Worktrees {
                        format: OutputFormat::Table,
                        relative: false,
                        jump: true,
                    },
                })
            );
        }

        assert!(Cli::parse(["-j"].map(Into::into)).is_err());
        assert!(Cli::parse(["-jwx"].map(Into::into)).is_err());
        assert!(Cli::parse(["--pick-worktree"].map(Into::into)).is_err());
    }

    #[test]
    fn parses_hidden_worktree_path_protocol_exactly() {
        assert_eq!(
            Cli::parse(["--worktree-paths0"].map(Into::into)),
            Ok(Cli {
                config_path: None,
                verbose: false,
                command: Command::WorktreePaths0,
            })
        );
        assert!(!HELP.contains("--worktree-paths0"));
        assert!(Cli::parse(["--worktree-paths"].map(Into::into)).is_err());
        assert!(Cli::parse(["--worktree-paths0", "-w"].map(Into::into)).is_err());
        assert!(Cli::parse(["--worktree-paths0", "target"].map(Into::into)).is_err());
        assert!(Cli::parse(["--worktree-paths0", "--"].map(Into::into)).is_err());
    }

    #[test]
    fn parses_hidden_key_binding_zoxide_protocol() {
        assert_eq!(
            Cli::parse(
                [
                    "--internal-key-binding-zoxide",
                    "/tools/zoxide",
                    "/tools/fzf"
                ]
                .map(Into::into)
            )
            .unwrap()
            .command,
            Command::KeyBindingZoxide {
                zoxide: "/tools/zoxide".into(),
                fzf: "/tools/fzf".into()
            }
        );
        assert!(!HELP.contains("--internal-key-binding-zoxide"));
        for extra in [
            "target",
            "-w",
            "-jw",
            "--jump-worktree",
            "--worktree-paths0",
            "-z",
            "--setup-key-binding",
            "--preamp",
            "--",
        ] {
            assert!(
                Cli::parse(
                    ["--internal-key-binding-zoxide", "zoxide", "fzf", extra].map(Into::into)
                )
                .is_err()
            );
        }
        assert!(Cli::parse(["--internal-key-binding-zoxide", "zoxide"].map(Into::into)).is_err());
        assert!(
            Cli::parse(
                [
                    "--internal-key-binding-zoxide",
                    "zoxide",
                    "fzf",
                    "-C",
                    "config.toml"
                ]
                .map(Into::into)
            )
            .is_err()
        );
        assert!(
            Cli::parse(
                [
                    "--internal-key-binding-zoxide",
                    "zoxide",
                    "fzf",
                    "-o",
                    "out"
                ]
                .map(Into::into)
            )
            .is_err()
        );
    }

    #[test]
    fn jump_worktree_rejects_ignored_output_flags() {
        assert!(Cli::parse(["--jump-worktree", "-R"].map(Into::into)).is_err());
        assert!(Cli::parse(["-jw", "-f", "json"].map(Into::into)).is_err());
    }

    #[test]
    fn parses_config_init_and_mount_scan() {
        let init = Cli::parse(["-v", "config", "init", "--preamp"].map(Into::into)).unwrap();
        assert!(init.verbose);
        assert_eq!(init.command, Command::ConfigInit { preamp: true });

        let scan = Cli::parse(["mounts", "scan", "-f", "json"].map(Into::into)).unwrap();
        assert_eq!(
            scan.command,
            Command::MountsScan {
                format: OutputFormat::Json
            }
        );
    }

    #[test]
    fn validates_new_command_flags_and_literal_escape() {
        assert!(Cli::parse(["config", "init", "--format", "json"].map(Into::into)).is_err());
        assert!(Cli::parse(["mounts", "scan", "--preamp"].map(Into::into)).is_err());
        assert!(matches!(
            Cli::parse(["--", "mounts", "scan"].map(Into::into))
                .unwrap()
                .command,
            Command::Resolve { .. }
        ));
    }

    #[test]
    fn parses_all_integration_and_completion_shells() {
        for (name, shell) in [
            ("bash", Shell::Bash),
            ("zsh", Shell::Zsh),
            ("nu", Shell::Nu),
            ("powershell", Shell::Pwsh),
            ("pwsh", Shell::Pwsh),
        ] {
            assert_eq!(
                Cli::parse(["init", name].map(Into::into)).unwrap().command,
                Command::Init {
                    shell,
                    setup_key_binding: false,
                    output: None,
                }
            );
            assert_eq!(
                Cli::parse(["completions", name].map(Into::into))
                    .unwrap()
                    .command,
                Command::Completions {
                    shell,
                    output: None,
                }
            );
        }
    }

    #[test]
    fn rejects_unsupported_shell_setup_combinations() {
        assert!(Cli::parse(["init", "fish"].map(Into::into)).is_err());
        assert!(Cli::parse(["init", "nu", "--setup-key-binding"].map(Into::into)).is_err());
        assert_eq!(
            Cli::parse(["init", "powershell", "--setup-key-binding"].map(Into::into)),
            Ok(Cli {
                config_path: None,
                verbose: false,
                command: Command::Init {
                    shell: Shell::Pwsh,
                    setup_key_binding: true,
                    output: None,
                },
            })
        );
        assert!(Cli::parse(["completions", "bash", "-w"].map(Into::into)).is_err());
        assert!(matches!(
            Cli::parse(["--", "completions", "bash"].map(Into::into))
                .unwrap()
                .command,
            Command::Resolve { .. }
        ));
    }

    #[test]
    fn output_is_scoped_to_shell_generation() {
        for (command, flag) in [
            ("init", "-o"),
            ("init", "--output"),
            ("completions", "-o"),
            ("completions", "--output"),
        ] {
            let parsed = Cli::parse([command, "zsh", flag, "nested/source.zsh"].map(Into::into))
                .unwrap()
                .command;
            if command == "completions" {
                assert_eq!(
                    parsed,
                    Command::Completions {
                        shell: Shell::Zsh,
                        output: Some("nested/source.zsh".into()),
                    }
                );
                continue;
            }
            assert_eq!(
                parsed,
                Command::Init {
                    shell: Shell::Zsh,
                    setup_key_binding: false,
                    output: Some("nested/source.zsh".into()),
                }
            );
        }
        assert_eq!(
            Cli::parse(["init", "zsh", "--output"].map(Into::into)),
            Err("--output requires a path".into())
        );
        assert_eq!(
            Cli::parse(["-o", "out", "top"].map(Into::into)),
            Err("--output can only be used with init or completions".into())
        );
    }
}
