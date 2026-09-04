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
            resolver::resolve(&targets, resolver, &config)
                .map(|path| path.to_string_lossy().into_owned())
        }
        Command::Init {
            shell,
            setup_key_binding: false,
        } => Ok(shell::render(shell, cli.config_path.as_deref())),
        Command::Init {
            setup_key_binding: true,
            ..
        } => Err("key-binding setup is not implemented yet".into()),
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
            Err("worktree picker is not implemented yet".into())
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
