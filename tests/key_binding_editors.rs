#![cfg(unix)]

mod support;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use support::{TempDir, assert_success, cj, write_executable};

#[test]
fn real_line_editors_cycle_history_and_restore_the_original_line() {
    if Command::new("zsh").arg("--version").output().is_err() {
        assert!(env::var_os("CJ_REQUIRE_SHELLS").is_none());
        return;
    }
    let bash = env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|path| path.join("bash"))
        .find(|path| {
            Command::new(path)
                .args(["-c", "(( BASH_VERSINFO[0] >= 4 ))"])
                .status()
                .is_ok_and(|status| status.success())
        });
    for shell in ["bash", "zsh"] {
        if shell == "bash" && bash.is_none() {
            assert!(
                env::var_os("CJ_REQUIRE_SHELLS").is_none(),
                "Bash 4+ required for widget tests"
            );
            continue;
        }
        let temp = TempDir::new("key-editors-雪");
        let config = temp.path().join("config.toml");
        fs::write(&config, "[key-bindings]\nmacos = { key = 'ctrl-o', behaviors = ['cj'] }\nlinux = { key = 'ctrl-o', behaviors = ['cj'] }\n").unwrap();
        let generated = cj(temp.path(), temp.path())
            .arg("-C")
            .arg(&config)
            .args(["init", shell, "--setup-key-binding"])
            .output()
            .unwrap();
        assert_success(&generated);
        let mut script = String::from_utf8(generated.stdout).unwrap();
        script.push_str(
            r#"
_cj_history=("$CJ_TEST_OLDER" "$CJ_TEST_NEWER")
_test_record() {
    printf '%s\000' "$1" "$2" "$PWD" "${_cj_history[@]}" >> "$CJ_TEST_RESULTS"
}

"#,
        );
        if shell == "bash" {
            script.push_str(
                r#"
_test_widget() {
    _cj_key_widget
    _test_record "$READLINE_LINE" "$READLINE_POINT"
}
bind -x '"\C-o":_test_widget'
set -o emacs
PS1='CJ_EDITOR_READY> '
"#,
            );
        } else {
            script.push_str(
                r#"
zle-line-init() {
    BUFFER='code '
    CURSOR=5
    for attempt in 1 2 3; do
        zle _cj_key_widget
        _test_record "$BUFFER" "$CURSOR"
    done
    exit
}
zle -N zle-line-init
test_buffer=''
vared test_buffer
"#,
            );
        }
        let source = temp.path().join("editor.sh");
        fs::write(&source, script).unwrap();
        let results = temp.path().join("results");
        let older = temp.path().join("older 'quoted' 雪");
        let newer = temp.path().join("-newer folder");
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_cj"));
        let mut paths = vec![binary.parent().unwrap().to_path_buf()];
        paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
        let output = Command::new("zsh")
            .args([
                "-fc",
                r#"
zmodload zsh/zpty || exit 1
if [[ "$CJ_TEST_SHELL" == bash ]]; then
    zpty -b cj_editor "$CJ_TEST_BASH" --noprofile --rcfile "$CJ_TEST_SCRIPT" -i || exit 1
    for attempt in {1..200}; do
        if zpty -r cj_editor startup; then
            [[ "$startup" == *'CJ_EDITOR_READY> '* ]] && break
        fi
        sleep 0.02
    done
    zpty -w -n cj_editor $'code \x0f\x0f\x0f\x15exit\n'
else
    zpty -b cj_editor zsh -fi "$CJ_TEST_SCRIPT" || exit 1
fi
for attempt in {1..200}; do
    while zpty -r cj_editor output; do print -rn -- "$output"; done
    zpty -t cj_editor || break
    sleep 0.05
done
timed_out=0
zpty -t cj_editor && timed_out=1
while zpty -r cj_editor output; do print -rn -- "$output"; done
zpty -d cj_editor
exit $timed_out
"#,
            ])
            .current_dir(temp.path())
            .env("PATH", env::join_paths(paths).unwrap())
            .env("TERM", "xterm")
            .env("INPUTRC", "/dev/null")
            .env("CJ_TEST_SHELL", shell)
            .env(
                "CJ_TEST_BASH",
                bash.as_deref().unwrap_or(std::path::Path::new("bash")),
            )
            .env("CJ_TEST_SCRIPT", source)
            .env("CJ_TEST_RESULTS", &results)
            .env("CJ_TEST_OLDER", &older)
            .env("CJ_TEST_NEWER", &newer)
            .output()
            .unwrap();
        assert_success(&output);
        let bytes = fs::read(&results).unwrap_or_else(|error| {
            panic!(
                "{shell}: {error}; {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        let fields: Vec<_> = bytes
            .split(|byte| *byte == 0)
            .filter(|field| !field.is_empty())
            .map(|field| std::str::from_utf8(field).unwrap())
            .collect();
        assert_eq!(fields.len(), 15, "{shell}: {fields:?}");
        for (index, record) in fields.chunks_exact(5).enumerate() {
            let expected = match index {
                0 => format!("code '{}'", newer.to_str().unwrap()),
                1 => format!("code '{}'", older.to_str().unwrap().replace('\'', "'\\''")),
                _ => "code ".into(),
            };
            assert_eq!(record[0], expected, "{shell}");
            let cursor = expected.chars().count();
            assert_eq!(record[1], cursor.to_string(), "{shell}");
            assert_eq!(record[2], temp.path().to_str().unwrap(), "{shell}");
            assert_eq!(record[3], older.to_str().unwrap(), "{shell}");
            assert_eq!(record[4], newer.to_str().unwrap(), "{shell}");
        }
    }
}

#[test]
fn real_zle_picker_preserves_the_line_on_cancellation_and_errors() {
    if Command::new("zsh").arg("--version").output().is_err() {
        assert!(env::var_os("CJ_REQUIRE_SHELLS").is_none());
        return;
    }
    let temp = TempDir::new("key-zle-picker");
    let zoxide = temp.path().join("zoxide");
    let fzf = temp.path().join("fzf");
    write_executable(
        &zoxide,
        "if [ \"$2\" = --list ]; then printf '/candidate\\n'; else fzf; fi",
    );
    write_executable(
        &fzf,
        r#"
test -t 0 < /dev/tty
case "$CJ_TEST_PICK_STATUS" in
    0) printf '%s\n' "$CJ_TEST_SELECTION" ;;
    7) printf '%s\n' 'durable picker failure' >&2; exit 7 ;;
    *) exit 130 ;;
esac
"#,
    );
    let config = temp.path().join("config.toml");
    fs::write(&config, format!("[programs]\nzoxide = {}\nfzf = {}\n[key-bindings]\nmacos = {{ key = 'ctrl-o', behaviors = ['zoxide', 'cj'] }}\nlinux = {{ key = 'ctrl-o', behaviors = ['zoxide', 'cj'] }}\n", serde_json::to_string(zoxide.to_str().unwrap()).unwrap(), serde_json::to_string(fzf.to_str().unwrap()).unwrap())).unwrap();
    let generated = cj(temp.path(), temp.path())
        .arg("-C")
        .arg(&config)
        .args(["init", "zsh", "--setup-key-binding"])
        .output()
        .unwrap();
    assert_success(&generated);
    let mut source = String::from_utf8(generated.stdout).unwrap();
    source.push_str(
        r#"
_cj_history=('/fallback must not be used')
zle-line-init() {
    local outcome
    for outcome in 0 130 7; do
        export CJ_TEST_PICK_STATUS=$outcome
        BUFFER='code '
        CURSOR=2
        zle _cj_key_widget
        printf '%s\000' "$BUFFER" "$CURSOR" "$PWD" "${_cj_history[@]}" >> "$CJ_TEST_RESULTS"
    done
    exit
}
zle -N zle-line-init
test_buffer=''
vared test_buffer
"#,
    );
    let script = temp.path().join("widget.zsh");
    fs::write(&script, source).unwrap();
    fs::remove_file(&config).unwrap();
    let results = temp.path().join("results");
    let selection = temp.path().join("space's 雪 directory");
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_cj"));
    let mut paths = vec![binary.parent().unwrap().to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let output = Command::new("zsh")
        .args([
            "-fc",
            r#"
zmodload zsh/zpty || exit 1
zpty -b cj_editor zsh -fi "$CJ_TEST_SCRIPT" || exit 1
for attempt in {1..200}; do
    while zpty -r cj_editor output; do print -rn -- "$output"; done
    zpty -t cj_editor || break
    sleep 0.05
done
timed_out=0
zpty -t cj_editor && timed_out=1
while zpty -r cj_editor output; do print -rn -- "$output"; done
zpty -d cj_editor
exit $timed_out
"#,
        ])
        .current_dir(temp.path())
        .env("PATH", env::join_paths(paths).unwrap())
        .env("TERM", "xterm")
        .env("CJ_TEST_SCRIPT", &script)
        .env("CJ_TEST_RESULTS", &results)
        .env("CJ_TEST_SELECTION", &selection)
        .output()
        .unwrap();
    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("durable picker failure"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let bytes = fs::read(results).unwrap();
    let fields: Vec<_> = bytes
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .map(|value| std::str::from_utf8(value).unwrap())
        .collect();
    assert_eq!(fields.len(), 12);
    let quoted = format!(
        "code '{}'",
        selection.to_str().unwrap().replace('\'', "'\\''")
    );
    for (index, record) in fields.chunks_exact(4).enumerate() {
        assert_eq!(record[0], if index == 0 { &quoted } else { "code " });
        assert_eq!(
            record[1],
            if index == 0 {
                quoted.chars().count().to_string()
            } else {
                "2".into()
            }
        );
        assert_eq!(record[2], temp.path().to_str().unwrap());
        assert_eq!(record[3], "/fallback must not be used");
    }
}
