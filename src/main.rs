use std::ffi::OsString;
use std::fmt;
use std::process::ExitCode;

const HELP: &str = "\
cj - jump between useful directories

Usage: cj [OPTIONS] [TARGET]

Options:
  -h, --help       Print help
  -V, --version    Print version

Directory targets will arrive in a future release.";

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
}

#[derive(Debug, PartialEq)]
struct CliError(String);

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, CliError> {
    let args: Vec<_> = args.into_iter().collect();

    match args.as_slice() {
        [] => Ok(Command::Help),
        [argument] if argument == "-h" || argument == "--help" => Ok(Command::Help),
        [argument] if argument == "-V" || argument == "--version" => Ok(Command::Version),
        [argument] => Err(CliError(format!(
            "directory targets are not available yet: {}",
            argument.to_string_lossy()
        ))),
        _ => Err(CliError("expected at most one target".into())),
    }
}

fn run(args: impl IntoIterator<Item = OsString>) -> Result<&'static str, CliError> {
    match parse(args)? {
        Command::Help => Ok(HELP),
        Command::Version => Ok(concat!("cj ", env!("CARGO_PKG_VERSION"))),
    }
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_help() {
        assert_eq!(parse([]), Ok(Command::Help));
    }

    #[test]
    fn parses_version() {
        assert_eq!(parse(["--version".into()]), Ok(Command::Version));
    }

    #[test]
    fn rejects_targets_until_the_resolver_exists() {
        assert_eq!(
            parse(["top".into()]),
            Err(CliError(
                "directory targets are not available yet: top".into()
            ))
        );
    }
}
