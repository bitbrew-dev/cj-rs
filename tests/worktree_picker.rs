#![cfg(feature = "test-tools")]

mod support;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use support::{TempDir, assert_success, cj, write_config};

#[test]
fn early_picker_exit_preserves_cancellation_selection_and_failure() {
    let temp = TempDir::new("early-worktree-picker");
    let bin = temp.path().join("fake tools");
    fs::create_dir(&bin).expect("create fake tools directory");
    copy_tool(&bin, "git");
    let fzf = copy_tool(&bin, "fzf");
    let config = temp.path().join("config.toml");
    write_config(&config, Path::new("missing-zoxide"), &fzf);

    // Exceed pipe capacity so an exit without reading stdin reliably interrupts
    // the parent's write, instead of accidentally fitting in the pipe buffer.
    let mut records = Vec::new();
    for index in 0..4096 {
        let path = temp
            .path()
            .join(format!("tree-{index}-{}", "x".repeat(192)));
        records.extend_from_slice(
            format!(
                "worktree {}\0HEAD abc\0branch refs/heads/main\0\0",
                path.display()
            )
            .as_bytes(),
        );
    }
    assert!(records.len() > 1024 * 1024);
    let listing = temp.path().join("worktree-list");
    fs::write(&listing, records).expect("write large worktree list");
    let mut paths = vec![bin];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(paths).expect("join fake tools PATH");

    for (mode, selection) in [
        ("cancel", "1"),
        ("no-match", "1"),
        ("success", "1"),
        ("success", "invalid"),
        ("failure", "1"),
    ] {
        let output = cj(temp.path(), temp.path())
            .arg("-C")
            .arg(&config)
            .arg("--jump-worktree")
            .env("PATH", &path)
            .env("CJ_FAKE_WORKTREE_LIST", &listing)
            .env("CJ_FAKE_MODE", mode)
            .env("CJ_FAKE_SELECTION", selection)
            .env_remove("CJ_FAKE_TOOL")
            .env_remove("CJ_FAKE_STDIN") // Exit immediately without reading.
            .env_remove("CJ_FAKE_ARGS")
            .output()
            .expect("run early-exiting picker");
        match (mode, selection) {
            ("cancel" | "no-match", _) => {
                assert_success(&output);
                assert_eq!(output.stdout, b"\n");
                assert!(output.stderr.is_empty());
            }
            ("success", "1") => {
                assert_success(&output);
                let expected = temp.path().join(format!("tree-1-{}", "x".repeat(192)));
                assert_eq!(
                    output.stdout,
                    format!("{}\n", expected.display()).as_bytes()
                );
                assert!(output.stderr.is_empty());
            }
            ("success", "invalid") => {
                assert_eq!(output.status.code(), Some(2));
                assert!(output.stdout.is_empty());
                assert_eq!(
                    output.stderr,
                    b"cj: fzf returned an invalid worktree selection\n"
                );
            }
            ("failure", _) => {
                assert_eq!(output.status.code(), Some(2));
                assert!(output.stdout.is_empty());
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(stderr.contains("fake fzf failure\ncj: fzf exited with"));
                assert!(stderr.contains('7'));
                assert!(!stderr.contains("cannot write fzf input"));
            }
            _ => unreachable!(),
        }
    }
}

fn copy_tool(directory: &Path, name: &str) -> PathBuf {
    let path = directory.join(format!("{name}{}", env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_cj-test-tool"), &path).expect("copy fake tool");
    path
}
