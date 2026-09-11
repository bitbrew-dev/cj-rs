mod cli;
mod completions;
mod config;
mod config_init;
mod key_bindings;
mod key_bindings_nu;
mod key_history;
mod key_zoxide;
mod mounts;
mod path_bytes;
mod powershell;
mod resolver;
mod shell;
#[cfg(windows)]
mod windows_mounts;
mod worktree;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use cli::{Cli, Command};

struct Execution {
    stdout: Vec<u8>,
    stderr: String,
    write_stdout: bool,
    trailing_newline: bool,
}

impl Execution {
    fn stdout(stdout: String) -> Self {
        Self {
            stdout: stdout.into_bytes(),
            stderr: String::new(),
            write_stdout: true,
            trailing_newline: true,
        }
    }

    fn raw(stdout: Vec<u8>) -> Self {
        Self {
            stdout,
            stderr: String::new(),
            write_stdout: true,
            trailing_newline: false,
        }
    }

    fn silent() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: String::new(),
            write_stdout: false,
            trailing_newline: false,
        }
    }

    fn path(path: PathBuf) -> Result<Self, String> {
        Ok(Self {
            stdout: path_bytes::output_bytes(&path)?.into_owned(),
            stderr: String::new(),
            write_stdout: true,
            trailing_newline: true,
        })
    }

    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(&self.stdout)?;
        if self.trailing_newline {
            writer.write_all(b"\n")?;
        }
        Ok(())
    }
}

fn generated(source: String, output: Option<PathBuf>) -> Result<Execution, String> {
    let Some(path) = output else {
        return Ok(Execution::stdout(source));
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "cannot create output directory {}: {error}",
                parent.display()
            )
        })?;
    }
    std::fs::write(&path, format!("{source}\n"))
        .map_err(|error| format!("cannot write output {}: {error}", path.display()))?;
    Ok(Execution::silent())
}

fn execute(cli: Cli) -> Result<Execution, String> {
    let config_path = cli.config_path;
    let verbose = cli.verbose;
    match cli.command {
        Command::Help => Ok(Execution::stdout(cli::HELP.into())),
        Command::Version => Ok(Execution::stdout(
            concat!("cj ", env!("CARGO_PKG_VERSION")).into(),
        )),
        Command::KeyBindingZoxide { zoxide, fzf } => {
            key_zoxide::query_zoxide(&zoxide, &fzf).map(Execution::raw)
        }
        Command::Resolve { targets, resolver } => {
            let config = config::Config::load(config_path.as_deref())?;
            let navigation = resolver::NavigationContext::from_process()?;
            resolver::resolve(&targets, resolver, &config, &navigation).and_then(Execution::path)
        }
        Command::Init {
            shell,
            setup_key_binding,
            output,
        } => {
            let config = config::Config::load(config_path.as_deref())?;
            let source = shell::render(
                shell,
                config_path.as_deref(),
                setup_key_binding.then(|| config.key_binding()),
                &config,
            )?;
            generated(source, output)
        }
        Command::Completions { shell, output } => {
            let config = config::Config::load(config_path.as_deref())?;
            generated(completions::render(shell, &config), output)
        }
        Command::Worktrees {
            format,
            relative,
            jump: false,
        } => {
            let _config = config::Config::load(config_path.as_deref())?;
            let cwd = std::env::current_dir()
                .map_err(|error| format!("cannot read current directory: {error}"))?;
            worktree::render(&worktree::list()?, format, relative, &cwd).map(Execution::stdout)
        }
        Command::Worktrees { jump: true, .. } => {
            let config = config::Config::load(config_path.as_deref())?;
            worktree::pick(&worktree::list()?, &config.programs.fzf)?
                .map(Execution::path)
                .unwrap_or_else(|| Ok(Execution::stdout(String::new())))
        }
        Command::WorktreePaths0 => worktree::paths0(&worktree::list()?).map(Execution::raw),
        Command::MainWorktree => resolver::main_worktree().and_then(Execution::path),
        Command::MountsScan { format } => {
            let context = mounts::DiscoveryContext::from_process()?;
            let report = mounts::scan(&context);
            let rendered = match format {
                cli::OutputFormat::Table => mounts::render_table(&report, verbose),
                cli::OutputFormat::Json => mounts::render_json(&report, verbose)?,
            };
            Ok(Execution {
                stdout: rendered.stdout.into_bytes(),
                stderr: rendered.stderr,
                write_stdout: true,
                trailing_newline: true,
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
                stdout: format!("created {}", path.display()).into_bytes(),
                stderr,
                write_stdout: true,
                trailing_newline: true,
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
            let result = if output.write_stdout {
                let mut stdout = std::io::stdout().lock();
                output.write_to(&mut stdout)
            } else {
                Ok(())
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("cj: cannot write stdout: {error}");
                    ExitCode::from(2)
                }
            }
        }
        Err(error) => {
            eprintln!("cj: {error}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Execution;

    #[test]
    fn raw_execution_does_not_append_a_line_feed() {
        let mut output = Vec::new();
        Execution::raw(b"first\0second\0".to_vec())
            .write_to(&mut output)
            .unwrap();
        assert_eq!(output, b"first\0second\0");
    }
}
