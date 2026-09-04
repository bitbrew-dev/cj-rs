mod cli;
mod config;
mod resolver;
mod shell;
mod worktree;

use std::process::ExitCode;

use cli::{Cli, Command};

fn execute(cli: Cli) -> Result<String, String> {
    match cli.command {
        Command::Help => Ok(cli::HELP.into()),
        Command::Version => Ok(concat!("cj ", env!("CARGO_PKG_VERSION")).into()),
        Command::Resolve { targets, resolver } => {
            let config = config::Config::load(cli.config_path.as_deref())?;
            let navigation = resolver::NavigationContext::from_process()?;
            resolver::resolve(&targets, resolver, &config, &navigation)
                .map(|path| path.to_string_lossy().into_owned())
        }
        Command::Init {
            shell,
            setup_key_binding,
        } => {
            let config = config::Config::load(cli.config_path.as_deref())?;
            shell::render(
                shell,
                cli.config_path.as_deref(),
                setup_key_binding.then(|| config.key_binding()),
                &config.tickers,
            )
        }
        Command::Worktrees {
            format,
            relative,
            pick: false,
        } => {
            let _config = config::Config::load(cli.config_path.as_deref())?;
            let cwd = std::env::current_dir()
                .map_err(|error| format!("cannot read current directory: {error}"))?;
            worktree::render(&worktree::list()?, format, relative, &cwd)
        }
        Command::Worktrees { pick: true, .. } => {
            let config = config::Config::load(cli.config_path.as_deref())?;
            Ok(worktree::pick(&worktree::list()?, &config.programs.fzf)?
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default())
        }
    }
}

fn main() -> ExitCode {
    match Cli::parse(std::env::args_os().skip(1)).and_then(execute) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cj: {error}");
            ExitCode::from(2)
        }
    }
}
