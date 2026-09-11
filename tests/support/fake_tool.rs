use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let tool = env::var("CJ_FAKE_TOOL").unwrap_or_else(|_| {
        env::current_exe()
            .ok()
            .and_then(|path| {
                path.file_stem()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default()
    });
    let mode = env::var("CJ_FAKE_MODE").unwrap_or_default();
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if let Some(path) = env::var_os("CJ_FAKE_ARGS") {
        let mut output = fs::File::create(path).expect("create argument recording");
        for argument in args {
            output
                .write_all(argument.to_string_lossy().as_bytes())
                .and_then(|()| output.write_all(&[0]))
                .expect("record argument");
        }
    }
    match (tool.as_str(), mode.as_str()) {
        ("git", _) => {
            let path = env::var_os("CJ_FAKE_WORKTREE_LIST").expect("CJ_FAKE_WORKTREE_LIST");
            io::stdout()
                .write_all(&fs::read(path).expect("read fake worktree list"))
                .expect("write fake worktree list");
            ExitCode::SUCCESS
        }
        ("zoxide", "success") => {
            println!("{}", env::var("CJ_FAKE_DEST").expect("CJ_FAKE_DEST"));
            ExitCode::SUCCESS
        }
        ("zoxide", "failure") => {
            eprintln!("fake zoxide failure");
            ExitCode::from(9)
        }
        ("fzf", "success") => {
            record_stdin();
            let index = env::var("CJ_FAKE_SELECTION").expect("CJ_FAKE_SELECTION");
            print!("{index}\tselected\0");
            ExitCode::SUCCESS
        }
        ("fzf", "no-match") => {
            record_stdin();
            ExitCode::from(1)
        }
        ("fzf", "cancel") => {
            record_stdin();
            ExitCode::from(130)
        }
        ("fzf", "failure") => {
            record_stdin();
            eprintln!("fake fzf failure");
            ExitCode::from(7)
        }
        _ => {
            eprintln!("unknown fake tool or mode");
            ExitCode::from(64)
        }
    }
}

fn record_stdin() {
    let Some(path) = env::var_os("CJ_FAKE_STDIN") else {
        return;
    };
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input).expect("read stdin");
    fs::write(path, input).expect("record stdin");
}
