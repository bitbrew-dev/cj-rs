mod cli;
mod completions;
mod config;
mod config_init;
mod mounts;
mod resolver;
mod shell;
#[cfg(windows)]
mod windows_mounts;
mod worktree;

use std::process::ExitCode;

use cli::{Cli, Command};

struct Execution {
    stdout: String,
    stderr: String,
}

impl Execution {
    fn stdout(stdout: String) -> Self {
        Self {
            stdout,
            stderr: String::new(),
        }
    }
}

fn execute(cli: Cli) -> Result<Execution, String> {
    let config_path = cli.config_path;
    let verbose = cli.verbose;
    match cli.command {
        Command::Help => Ok(Execution::stdout(cli::HELP.into())),
        Command::Version => Ok(Execution::stdout(
            concat!("cj ", env!("CARGO_PKG_VERSION")).into(),
        )),
        Command::Resolve { targets, resolver } => {
            let config = config::Config::load(config_path.as_deref())?;
            let navigation = resolver::NavigationContext::from_process()?;
            resolver::resolve(&targets, resolver, &config, &navigation)
                .map(|path| path.to_string_lossy().into_owned())
                .map(Execution::stdout)
        }
        Command::Init {
            shell,
            setup_key_binding,
        } => {
            let config = config::Config::load(config_path.as_deref())?;
            shell::render(
                shell,
                config_path.as_deref(),
                setup_key_binding.then(|| config.key_binding()),
                &config,
            )
            .map(Execution::stdout)
        }
        Command::Completions { shell } => {
            let config = config::Config::load(config_path.as_deref())?;
            Ok(Execution::stdout(completions::render(shell, &config)))
        }
        Command::Worktrees {
            format,
            relative,
            pick: false,
        } => {
            let _config = config::Config::load(config_path.as_deref())?;
            let cwd = std::env::current_dir()
                .map_err(|error| format!("cannot read current directory: {error}"))?;
            worktree::render(&worktree::list()?, format, relative, &cwd).map(Execution::stdout)
        }
        Command::Worktrees { pick: true, .. } => {
            let config = config::Config::load(config_path.as_deref())?;
            Ok(Execution::stdout(
                worktree::pick(&worktree::list()?, &config.programs.fzf)?
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        Command::MountsScan { format } => {
            let context = mounts::DiscoveryContext::from_process()?;
            let report = mounts::scan(&context);
            let rendered = match format {
                cli::OutputFormat::Table => mounts::render_table(&report, verbose),
                cli::OutputFormat::Json => mounts::render_json(&report, verbose)?,
            };
            Ok(Execution {
                stdout: rendered.stdout,
                stderr: rendered.stderr,
            })
        }
        Command::ConfigInit { preamp } => {
            let context = preamp
                .then(mounts::DiscoveryContext::from_process)
                .transpose()?;
            let report = context.as_ref().map(mounts::scan);
            let discovered = report
                .as_ref()
                .map(|report| report.ready_mounts().collect::<Vec<_>>())
                .unwrap_or_default();
            let path = config_init::create(config_path.as_deref(), discovered)?;
            let stderr = report
                .as_ref()
                .map(|report| mounts::render_table(report, verbose).stderr)
                .unwrap_or_default();
            Ok(Execution {
                stdout: format!("created {}", path.display()),
                stderr,
            })
        }
    }
}

fn main() -> ExitCode {
    match Cli::parse(std::env::args_os().skip(1)).and_then(execute) {
        Ok(output) => {
            if !output.stderr.is_empty() {
                eprintln!("{}", output.stderr);
            }
            println!("{}", output.stdout);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cj: {error}");
            ExitCode::from(2)
        }
    }
}
