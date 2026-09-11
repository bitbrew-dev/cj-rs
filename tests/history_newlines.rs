#![cfg(unix)]
mod support;

use std::env;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;
use support::{TempDir, assert_success, cj, write_executable};

/// Every fixture includes the trimmed sibling, so corruption cannot hide behind cd failure.
fn exercise(script: &str, check: impl Fn(&Path, &[Vec<u8>])) {
    let temp = TempDir::new("history-newlines");
    for relative in [
        "name",
        "name\n",
        "name\n\n",
        "parent",
        "parent\n\n/child",
        "parent\n\n/child\n",
    ] {
        fs::create_dir_all(temp.path().join(relative)).unwrap();
    }
    let zoxide = temp.path().join("fake-zoxide");
    write_executable(
        &zoxide,
        "if [ \"${CJ_FAIL-}\" = yes ]; then exit 19; fi\nprintf '%s\\n' \"$CJ_TEST_DEST\"",
    );
    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        format!("[programs]\nzoxide = {:?}\n", zoxide.to_str().unwrap()),
    )
    .unwrap();
    let mut paths = vec![
        Path::new(env!("CARGO_BIN_EXE_cj"))
            .parent()
            .unwrap()
            .to_path_buf(),
    ];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(paths).unwrap();
    let mut exercised = 0;
    for shell in ["bash", "zsh"] {
        let available = Command::new(shell)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        assert!(
            available || env::var_os("CJ_REQUIRE_SHELLS").is_none(),
            "required shell unavailable: {shell}"
        );
        if !available {
            continue;
        }
        // A preceding shell may have removed a history destination.
        fs::create_dir_all(temp.path().join("name\n\n")).unwrap();
        let source = cj(temp.path(), temp.path())
            .arg("-C")
            .arg(&config)
            .args(["init", shell])
            .output()
            .unwrap();
        assert_success(&source);
        let integration = temp.path().join("init.sh");
        fs::write(&integration, source.stdout).unwrap();
        let setup = r#"set -e
. "$CJ_SOURCE"
snapshot() {
    printf '%s\0' "$PWD" "${#_cj_history[@]}"
    if (( ${#_cj_history[@]} )); then printf '%s\0' "${_cj_history[@]}"; fi
}
"#;
        let output = Command::new(shell)
            .args(if shell == "bash" {
                &["--noprofile", "--norc", "-c"][..]
            } else {
                &["-f", "-c"][..]
            })
            .arg(format!("{setup}\n{script}"))
            .current_dir(temp.path())
            .env("PATH", &path)
            .env("CJ_SOURCE", &integration)
            .env("CJ_ROOT", temp.path())
            .env("CJ_ONE", temp.path().join("name\n"))
            .env("CJ_TWO", temp.path().join("name\n\n"))
            .env("CJ_PARENT", temp.path().join("parent\n\n"))
            .env("CJ_CHILD", temp.path().join("parent\n\n/child\n"))
            .env("CJ_TEST_DEST", temp.path().join("name\n\n"))
            .env("HOME", temp.path())
            .env_remove("CJ_FAIL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert_success(&output);
        assert!(output.stdout.ends_with(b"\0"), "{shell}: missing snapshot");
        let fields: Vec<_> = output.stdout[..output.stdout.len() - 1]
            .split(|byte| *byte == 0)
            .map(<[u8]>::to_vec)
            .collect();
        check(temp.path(), &fields);
        exercised += 1;
    }
    assert!(exercised > 0, "no POSIX shell available");
}

fn snapshot(root: &Path, cwd: &str, history: &[&str]) -> Vec<Vec<u8>> {
    let bytes = |relative: &str| {
        if relative.is_empty() {
            root.to_path_buf()
        } else {
            root.join(relative)
        }
        .as_os_str()
        .as_bytes()
        .to_vec()
    };
    let mut fields = vec![bytes(cwd), history.len().to_string().into_bytes()];
    fields.extend(history.iter().map(|relative| bytes(relative)));
    fields
}

#[test]
fn raw_direct_moves_and_back_preserve_one_or_more_trailing_newlines() {
    exercise(
        r#"
cd -r "$CJ_ONE"; snapshot
cd "$CJ_TWO"; snapshot
cd v; snapshot
cd v; snapshot
"#,
        |root, actual| {
            let expected = [
                snapshot(root, "name\n", &[""]),
                snapshot(root, "name\n\n", &["", "name\n"]),
                snapshot(root, "name\n", &[""]),
                snapshot(root, "", &[]),
            ]
            .concat();
            assert_eq!(actual, expected);
        },
    );
}

#[test]
fn up_records_exact_intermediate_parent_paths() {
    exercise(
        r#"
builtin cd "$CJ_CHILD"
cd '^^'; snapshot
cd v; snapshot
cd v; snapshot
"#,
        |root, actual| {
            let expected = [
                snapshot(root, "", &["parent\n\n/child\n", "parent\n\n"]),
                snapshot(root, "parent\n\n", &["parent\n\n/child\n"]),
                snapshot(root, "parent\n\n/child\n", &[]),
            ]
            .concat();
            assert_eq!(actual, expected);
        },
    );
}

#[test]
fn newline_noops_and_missing_history_destinations_preserve_the_stack() {
    exercise(
        r#"
cd "$CJ_TWO"
cd "$CJ_ONE"
cd .; cd -r .
if cd -r "$CJ_ROOT/missing" 2>/dev/null; then exit 1; fi
snapshot
builtin cd "$CJ_TWO"
cd v; snapshot
builtin cd "$CJ_ONE"
rmdir "$CJ_TWO"
if cd v 2>/dev/null; then exit 1; fi
snapshot
if cd vvv 2>/dev/null; then exit 1; else [[ $? == 2 ]]; fi
snapshot
"#,
        |root, actual| {
            let expected = [
                snapshot(root, "name\n", &["", "name\n\n"]),
                snapshot(root, "name\n\n", &["", "name\n\n"]),
                snapshot(root, "name\n", &["", "name\n\n"]),
                snapshot(root, "name\n", &["", "name\n\n"]),
            ]
            .concat();
            assert_eq!(actual, expected);
        },
    );
}

#[test]
fn resolver_output_preserves_path_newlines_and_failure_status() {
    exercise(
        r#"
cd -z project; snapshot
CJ_FAIL=yes
export CJ_FAIL
if cd -z project; then exit 1; else [[ $? == 2 ]]; fi
snapshot
cd v; snapshot
"#,
        |root, actual| {
            let expected = [
                snapshot(root, "name\n\n", &[""]),
                snapshot(root, "name\n\n", &[""]),
                snapshot(root, "", &[]),
            ]
            .concat();
            assert_eq!(actual, expected);
        },
    );
}

#[test]
fn failed_physical_cwd_capture_keeps_its_status_and_history() {
    exercise(
        r#"
cd "$CJ_ONE"
# Fail pwd deterministically before any native move can happen.
function builtin() { return 37; }
if cd -r "$CJ_TWO"; then exit 1; else [[ $? == 37 ]]; fi
snapshot
if cd v; then exit 1; else [[ $? == 37 ]]; fi
snapshot
if cd -z project; then exit 1; else [[ $? == 37 ]]; fi
snapshot
"#,
        |root, actual| {
            let expected = vec![snapshot(root, "name\n", &[""]); 3].concat();
            assert_eq!(actual, expected);
        },
    );
}
