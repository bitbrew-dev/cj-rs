mod cli;
mod config;

use std::process::ExitCode;

use cli::{Cli, Command};

fn execute(cli: Cli) -> Result<String, String> {
    match cli.command {
        Command::Help => Ok(cli::HELP.into()),
        Command::Version => Ok(concat!("cj ", env!("CARGO_PKG_VERSION")).into()),
        Command::Resolve { .. } | Command::Worktrees { .. } | Command::Init { .. } => {
            let _config = config::Config::load(cli.config_path.as_deref())?;
            Err("command is not implemented yet".into())
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
