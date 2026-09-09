#![cfg(unix)]

mod support;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use support::{TempDir, assert_success, cj, write_executable};

fn available() -> bool {
    let available = Command::new("nu").arg("--version").output().is_ok();
    assert!(available || std::env::var_os("CJ_REQUIRE_SHELLS").is_none());
    available
}

fn run(root: &Path, behaviors: &str, history: Value, actions: Value) -> Vec<Value> {
    let config = root.join("config.toml");
    let zoxide = toml_path(&root.join("zoxide"));
    let fzf = toml_path(&root.join("picker"));
    fs::write(&config, format!(
        "[programs]\nzoxide = '{zoxide}'\nfzf = '{fzf}'\n[key-bindings]\nmacos = {{ key = 'ctrl-o', behaviors = [{behaviors}] }}\nlinux = {{ key = 'ctrl-o', behaviors = [{behaviors}] }}\nwindows = {{ key = 'ctrl-o', behaviors = [{behaviors}] }}\n"
    )).unwrap();
    let generated = cj(root, root)
        .arg("-C")
        .arg(&config)
        .args(["init", "nu", "--setup-key-binding"])
        .output()
        .unwrap();
    assert_success(&generated);
    fs::write(root.join("init.nu"), generated.stdout).unwrap();
    fs::write(
        root.join("actions.json"),
        json!({"history": history, "actions": actions}).to_string(),
    )
    .unwrap();
    let binary = Path::new(env!("CARGO_BIN_EXE_cj"));
    let mut paths = vec![binary.parent().unwrap().to_owned()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let output = Command::new("python3")
        .arg("-c")
        .arg(include_str!("support/nu_key_binding_pty.py"))
        .arg(root)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .output()
        .unwrap();
    assert_success(&output);
    serde_json::from_slice(&output.stdout).unwrap()
}

fn toml_path(path: &Path) -> String {
    assert!(!path.to_string_lossy().contains('\''));
    path.to_string_lossy().into_owned()
}

#[test]
fn nushell_history_widget_cycles_and_cancels_in_reedline() {
    if !available() {
        return;
    }
    let temp = TempDir::new("nu-history-widget");
    let history = json!(["/old path's", "/new雪"]);
    let rows = run(
        temp.path(),
        "'cj'",
        history.clone(),
        json!([
            {"send": "echo é\u{1b}[D\u{f}", "record": true},
            {"send": "\u{f}", "record": true},
            {"send": "\u{f}", "record": true},
            {"send": "\u{f}", "record": true},
            {"send": "\u{3}"},
            {"send": "echo é\u{1b}[D\u{f}", "record": true},
            {"send": "X\u{f}", "record": true}
        ]),
    );
    assert_eq!(rows[0]["buffer"], "echo é r#'/new雪'#");
    assert_eq!(rows[1]["buffer"], "echo é r#'/old path's'#");
    assert_eq!(rows[2]["buffer"], "echo é");
    assert_eq!(rows[2]["cursor"], 5);
    assert_eq!(rows[3]["buffer"], rows[0]["buffer"]);
    assert_eq!(rows[4]["buffer"], rows[0]["buffer"]);
    assert_eq!(rows[5]["buffer"], "echo é r#'/new雪'#X r#'/new雪'#");
    for row in rows {
        assert_eq!(row["history"], history);
        assert_eq!(row["cwd"], temp.path().to_string_lossy().as_ref());
        assert_eq!(row["route"], "/sentinel/down/route");
    }
}

#[test]
fn nushell_widget_picker_owns_foreground_terminal_and_preserves_literal_path() {
    if !available() {
        return;
    }
    let temp = TempDir::new("nu-picker-widget");
    write_executable(
        &temp.path().join("zoxide"),
        r#"if [ "$2" = --list ]; then printf '/candidate\n'; else exec fzf; fi"#,
    );
    let target = "/a path's '$()雪\nend\n";
    let picker = format!(
        "exec python3 -c {}",
        shell_quote(&format!(
            "import os,sys; fd=os.open('/dev/tty',os.O_RDWR); assert os.tcgetpgrp(fd)==os.getpgrp(); assert os.isatty(2); sys.stdout.write({target:?}+'\\n')"
        ))
    );
    write_executable(&temp.path().join("picker"), &picker);
    let rows = run(
        temp.path(),
        "'zoxide', 'cj'",
        json!(["/fallback"]),
        json!([
            {"send":"echo base\u{f}","record":true}
        ]),
    );
    assert_eq!(rows[0]["buffer"], format!("echo base r#'{target}'#"));
    assert_eq!(rows[0]["cwd"], temp.path().to_string_lossy().as_ref());
    assert_eq!(rows[0]["history"], json!(["/fallback"]));
}

#[test]
fn nushell_widget_empty_missing_cancel_and_failure_preserve_buffer() {
    if !available() {
        return;
    }
    for (label, script, behaviors, expected) in [
        ("empty-history", None, "'cj'", "echo base"),
        (
            "missing-fallback",
            None,
            "'zoxide', 'cj'",
            "echo base r#'/fallback'#",
        ),
        (
            "empty-fallback",
            Some("exit 0"),
            "'zoxide', 'cj'",
            "echo base r#'/fallback'#",
        ),
        (
            "cancel",
            Some("if [ \"$2\" = --list ]; then printf '/candidate\\n'; else exit 130; fi"),
            "'zoxide', 'cj'",
            "echo base",
        ),
        ("failure", Some("exit 2"), "'zoxide', 'cj'", "echo base"),
    ] {
        let temp = TempDir::new(&format!("nu-widget-{label}"));
        if let Some(script) = script {
            write_executable(&temp.path().join("zoxide"), script);
            write_executable(&temp.path().join("picker"), "exit 0");
        }
        let history = if label == "empty-history" {
            json!([])
        } else {
            json!(["/fallback"])
        };
        let rows = run(
            temp.path(),
            behaviors,
            history.clone(),
            json!([
                {"send":"echo base\u{f}","record":true}
            ]),
        );
        assert_eq!(rows[0]["buffer"], expected, "{label}");
        assert_eq!(rows[0]["history"], history, "{label}");
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
