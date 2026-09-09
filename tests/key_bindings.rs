#![cfg(unix)]
mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use support::{TempDir, assert_success, cj, write_executable};

struct Fixture {
    temp: TempDir,
    zoxide: std::path::PathBuf,
    fzf: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new("key-binding");
        let zoxide = temp.path().join("custom zoxide's executable");
        let fzf = temp.path().join("custom fzf's executable");
        write_executable(
            &zoxide,
            r#"
case "$*" in
  'query --list') printf '%s' "${CJ_TEST_CANDIDATES-/somewhere}"; exit "${CJ_TEST_LIST_STATUS-0}" ;;
  'query --interactive')
    printf '%s\n' "$*" >> "$CJ_TEST_LOG"
    fzf
    ;;
  *) echo 'unexpected zoxide arguments' >&2; exit 2 ;;
esac"#,
        );
        write_executable(
            &fzf,
            r#"
printf '%s' "${_ZO_FZF_OPTS-}" > "$CJ_TEST_OPTS"
printf '%s' "${CJ_TEST_PICK-}"
if [ "${CJ_TEST_PICK_STATUS-0}" = 7 ]; then echo 'picker exploded' >&2; fi
exit "${CJ_TEST_PICK_STATUS-0}""#,
        );
        Self { temp, zoxide, fzf }
    }
    fn command(&self) -> Command {
        let mut command = cj(self.temp.path(), self.temp.path());
        command
            .args(["--internal-key-binding-zoxide"])
            .arg(&self.zoxide)
            .arg(&self.fzf);
        self.environment(&mut command);
        command
    }
    fn environment(&self, command: &mut Command) {
        command
            .env("CJ_TEST_LOG", self.temp.path().join("calls"))
            .env("CJ_TEST_OPTS", self.temp.path().join("opts"));
    }
    fn source(&self, shell: &str, behaviors: &str) -> String {
        self.source_with_chord(shell, behaviors, "ctrl-o")
    }
    fn source_with_chord(&self, shell: &str, behaviors: &str, chord: &str) -> String {
        let config = self.temp.path().join("config.toml");
        let text = format!(
            "[programs]\nzoxide = {}\nfzf = {}\n[key-bindings]\nmacos = {{ key = '{chord}', behaviors = [{behaviors}] }}\nlinux = {{ key = '{chord}', behaviors = [{behaviors}] }}\nwindows = {{ key = '{chord}', behaviors = [{behaviors}] }}\n",
            serde_json::to_string(self.zoxide.to_str().unwrap()).unwrap(),
            serde_json::to_string(self.fzf.to_str().unwrap()).unwrap()
        );
        fs::write(&config, text).unwrap();
        let output = cj(self.temp.path(), self.temp.path())
            .arg("-C")
            .arg(&config)
            .args(["init", shell, "--setup-key-binding"])
            .output()
            .unwrap();
        assert_success(&output);
        // Keypresses use the initialized settings even when the file disappears.
        fs::remove_file(config).unwrap();
        String::from_utf8(output.stdout).unwrap()
    }
    fn widget(
        &self,
        shell: &str,
        behaviors: &str,
        status: &str,
        candidates: &str,
        prefix: &str,
        selection: &str,
    ) -> Output {
        let source = self.source(shell, behaviors);
        let script = self.temp.path().join("widget.sh");
        let (line, cursor) = if shell == "bash" {
            ("READLINE_LINE", "READLINE_POINT")
        } else {
            ("BUFFER", "CURSOR")
        };
        fs::write(
            &script,
            format!(
                r#"{source}
_cj_history=('/old history' '/new history')
{line}="$CJ_TEST_PREFIX"
{cursor}=1
_cj_key_widget
printf '%s\0%s\0%s\0' "${line}" "${cursor}" "$PWD"
printf '%s\0' "${{_cj_history[@]}}"
"#
            ),
        )
        .unwrap();
        let mut command = Command::new(shell);
        command
            .arg(&script)
            .current_dir(self.temp.path())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    Path::new(env!("CARGO_BIN_EXE_cj"))
                        .parent()
                        .unwrap()
                        .display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("CJ_TEST_PREFIX", prefix)
            .env("CJ_TEST_PICK", selection)
            .env("CJ_TEST_PICK_STATUS", status)
            .env("CJ_TEST_CANDIDATES", candidates);
        self.environment(&mut command);
        command.output().unwrap()
    }
}
fn shells() -> impl Iterator<Item = &'static str> {
    ["bash", "zsh"]
        .into_iter()
        .filter(|shell| Command::new(shell).arg("--version").output().is_ok())
}
fn legacy_bash() -> bool {
    let output = Command::new("bash")
        .args(["-c", "printf '%s' \"${BASH_VERSINFO[0]}\""])
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .parse::<u32>()
        .unwrap()
        < 5
}
fn fields(output: &Output) -> Vec<&str> {
    std::str::from_utf8(&output.stdout)
        .unwrap()
        .split('\0')
        .collect()
}

#[test]
fn helper_runs_configured_programs_and_preserves_zoxide_options_and_path() {
    let fixture = Fixture::new();
    let path = "/space's dir/Unicode 台灣/$literal\n\n";
    let output = fixture
        .command()
        .env("CJ_TEST_PICK", format!("{path}\n"))
        .env("_ZO_FZF_OPTS", "--height=80% --no-sort")
        .output()
        .unwrap();
    assert_success(&output);
    assert_eq!(output.stdout, format!("selected\n{path}").as_bytes());
    assert_eq!(
        fs::read_to_string(fixture.temp.path().join("opts")).unwrap(),
        "--height=80% --no-sort"
    );
    assert_eq!(
        fs::read_to_string(fixture.temp.path().join("calls")).unwrap(),
        "query --interactive\n"
    );
}

#[test]
fn helper_distinguishes_missing_empty_cancelled_and_failed_sources() {
    let fixture = Fixture::new();
    let empty = fixture
        .command()
        .env("CJ_TEST_CANDIDATES", "")
        .output()
        .unwrap();
    assert_success(&empty);
    assert_eq!(empty.stdout, b"empty\n");
    assert!(!fixture.temp.path().join("calls").exists());
    let cancelled = fixture
        .command()
        .env("CJ_TEST_PICK_STATUS", "130")
        .output()
        .unwrap();
    assert_success(&cancelled);
    assert_eq!(cancelled.stdout, b"cancelled\n");
    let failed = fixture
        .command()
        .env("CJ_TEST_PICK_STATUS", "7")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("picker exploded"));
    let list_failure = fixture
        .command()
        .env("CJ_TEST_LIST_STATUS", "3")
        .output()
        .unwrap();
    assert!(!list_failure.status.success());
    assert!(String::from_utf8_lossy(&list_failure.stderr).contains("zoxide query failed"));
    fs::remove_file(&fixture.fzf).unwrap();
    let missing = fixture.command().output().unwrap();
    assert_success(&missing);
    assert!(
        String::from_utf8_lossy(&missing.stdout)
            .starts_with("unavailable\nfzf executable not found:")
    );
}

#[test]
fn widgets_append_quoted_paths_and_preserve_cwd_and_history() {
    for shell in shells() {
        for (prefix, target) in [
            ("code --wait", "/space's dir/台灣;$()"),
            ("", "/selected"),
            ("cd ", "-leading-dash"),
            ("open café", "/last\n\n"),
        ] {
            let fixture = Fixture::new();
            let output = fixture.widget(
                shell,
                "'zoxide', 'cj'",
                "0",
                "/candidate",
                prefix,
                &format!("{target}\n"),
            );
            assert_success(&output);
            let fields = fields(&output);
            let target = if target.starts_with('-') {
                format!("./{target}")
            } else {
                target.into()
            };
            let separator = if prefix.is_empty() || prefix.ends_with(' ') {
                ""
            } else {
                " "
            };
            let expected = format!("{prefix}{separator}'{}'", target.replace('\'', "'\\''"));
            assert_eq!(fields[0], expected, "{shell}");
            let cursor = if shell == "bash" && legacy_bash() {
                expected.len()
            } else {
                expected.chars().count()
            };
            assert_eq!(fields[1], cursor.to_string());
            assert_eq!(fields[2], fixture.temp.path().to_str().unwrap());
            assert_eq!(&fields[3..5], ["/old history", "/new history"]);
        }
    }
}

#[test]
fn cancellation_and_query_failure_stop_the_chain_without_edits() {
    for shell in shells() {
        for status in ["130", "7"] {
            let fixture = Fixture::new();
            let output = fixture.widget(
                shell,
                "'zoxide', 'cj'",
                status,
                "/candidate",
                "code original",
                "",
            );
            assert_success(&output);
            let fields = fields(&output);
            assert_eq!(&fields[..2], ["code original", "1"], "{shell}, {status}");
            assert_eq!(fields[2], fixture.temp.path().to_str().unwrap());
            if status == "7" {
                assert!(String::from_utf8_lossy(&output.stderr).contains("picker exploded"));
            }
        }
    }
}

#[test]
fn unavailable_or_empty_source_falls_through_in_configured_order() {
    for shell in shells() {
        for missing in [false, true] {
            let fixture = Fixture::new();
            if missing {
                fs::remove_file(&fixture.fzf).unwrap();
            }
            let output = fixture.widget(shell, "'zoxide', 'cj'", "0", "", "code", "");
            assert_success(&output);
            assert_eq!(fields(&output)[0], "code '/new history'", "{shell}");
            assert!(!fixture.temp.path().join("calls").exists());
        }
        let fixture = Fixture::new();
        let output = fixture.widget(
            shell,
            "'cj', 'zoxide'",
            "0",
            "/candidate",
            "code",
            "/selected\n",
        );
        assert_success(&output);
        assert_eq!(fields(&output)[0], "code '/new history'");
        assert!(!fixture.temp.path().join("calls").exists());
    }
}

#[test]
fn missing_dependency_is_diagnostic_when_no_fallback_edits() {
    for shell in shells() {
        let fixture = Fixture::new();
        fs::remove_file(&fixture.zoxide).unwrap();
        let output = fixture.widget(shell, "'zoxide'", "0", "", "cd", "");
        assert_success(&output);
        assert_eq!(&fields(&output)[..2], ["cd", "1"]);
        assert!(String::from_utf8_lossy(&output.stderr).contains("zoxide executable not found:"));
    }
}

#[test]
fn installed_zoxide_owns_interactive_selection_and_cancel_status() {
    if Command::new("zoxide").arg("--version").output().is_err() {
        return;
    }
    let fixture = Fixture::new();
    let directory = fixture.temp.path().join("real zoxide's 台灣 directory");
    fs::create_dir(&directory).unwrap();
    let data = fixture.temp.path().join("zoxide-data");
    let added = Command::new("zoxide")
        .arg("add")
        .arg(&directory)
        .env("_ZO_DATA_DIR", &data)
        .env_remove("_ZO_EXCLUDE_DIRS")
        .output()
        .unwrap();
    assert_success(&added);
    write_executable(
        &fixture.fzf,
        r#"
printf '%s' "${FZF_DEFAULT_OPTS-}" > "$CJ_TEST_OPTS"
if [ "${CJ_TEST_CANCEL-0}" = 1 ]; then exit 130; fi
tr '\000' '\n' | head -n 1
"#,
    );
    for cancel in [false, true] {
        let mut command = cj(fixture.temp.path(), fixture.temp.path());
        command
            .arg("--internal-key-binding-zoxide")
            .arg("zoxide")
            .arg(&fixture.fzf)
            .env("_ZO_DATA_DIR", &data)
            .env_remove("_ZO_EXCLUDE_DIRS")
            .env("_ZO_FZF_OPTS", "--height=80%")
            .env("CJ_TEST_CANCEL", if cancel { "1" } else { "0" });
        fixture.environment(&mut command);
        let output = command.output().unwrap();
        assert_success(&output);
        if cancel {
            assert_eq!(output.stdout, b"cancelled\n");
        } else {
            assert_eq!(
                output.stdout,
                format!("selected\n{}", directory.display()).as_bytes()
            );
        }
        assert_eq!(
            fs::read_to_string(fixture.temp.path().join("opts")).unwrap(),
            "--height=80%"
        );
    }
}

#[test]
fn powershell_dispatcher_preserves_buffers_and_orders_behaviors() {
    if Command::new("pwsh").arg("-Version").output().is_err() {
        return;
    }
    // Exercise PowerShell parsing and dispatch with an in-memory line-editor
    // adapter. The native editor lifecycle is covered by the PTY tests.
    for (behaviors, status, candidates, expected) in [
        ("'zoxide', 'cj'", "0", "/candidate", "code '/a''s 雪/‘’'"),
        ("'zoxide', 'cj'", "130", "/candidate", "code"),
        ("'zoxide', 'cj'", "7", "/candidate", "code"),
        ("'zoxide', 'cj'", "0", "", "code '/new history'"),
        ("'cj', 'zoxide'", "0", "/candidate", "code '/new history'"),
    ] {
        let fixture = Fixture::new();
        let source = fixture.source("powershell", behaviors);
        let script = fixture.temp.path().join("widget.ps1");
        fs::write(&script, format!(r#"
Import-Module Microsoft.PowerShell.Utility
Import-Module Microsoft.PowerShell.Management
$ErrorActionPreference = 'Stop'
$PSModuleAutoLoadingPreference = 'None'
Add-Type -TypeDefinition @'
namespace Microsoft.PowerShell {{
    public static class PSConsoleReadLine {{
        public static string Buffer = "code";
        public static int Cursor = 1;
        public static void GetBufferState(ref string buffer, ref int cursor) {{ buffer = Buffer; cursor = Cursor; }}
        public static void Replace(int start, int length, string text) {{ Buffer = text; }}
        public static void SetCursorPosition(int cursor) {{ Cursor = cursor; }}
    }}
}}
'@
{source}
$global:__cj_history = @('/old history', '/new history')
Invoke-CjKeyWidget
[Console]::WriteLine([Microsoft.PowerShell.PSConsoleReadLine]::Buffer)
[Console]::WriteLine([Microsoft.PowerShell.PSConsoleReadLine]::Cursor)
[Console]::WriteLine((Get-Location).Path)
"#)).unwrap();
        let mut command = Command::new("pwsh");
        command
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
            .arg(&script)
            .current_dir(fixture.temp.path())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    Path::new(env!("CARGO_BIN_EXE_cj"))
                        .parent()
                        .unwrap()
                        .display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("CJ_TEST_PICK", "/a's 雪/‘’\n")
            .env("CJ_TEST_PICK_STATUS", status)
            .env("CJ_TEST_CANDIDATES", candidates);
        fixture.environment(&mut command);
        let output = command.output().unwrap();
        assert_success(&output);
        let text = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<_> = text.lines().collect();
        let expected = expected.replace('‘', "‘‘").replace('’', "’’");
        assert_eq!(
            lines[0],
            expected,
            "{status}, {behaviors}, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            lines[1],
            if status == "130" || status == "7" {
                1
            } else {
                expected.encode_utf16().count()
            }
            .to_string()
        );
        assert_eq!(lines[2], fixture.temp.path().to_str().unwrap());
    }
}

#[test]
fn sourcing_again_updates_only_the_binding_owned_by_cj() {
    let bash = if Path::new("/opt/homebrew/bin/bash").exists() {
        "/opt/homebrew/bin/bash"
    } else {
        "bash"
    };
    for (shell, executable) in [("bash", bash), ("zsh", "zsh")] {
        if Command::new(executable).arg("--version").output().is_err() {
            continue;
        }
        if shell == "bash"
            && !Command::new(executable)
                .args(["-c", "(( BASH_VERSINFO[0] >= 4 ))"])
                .status()
                .unwrap()
                .success()
        {
            continue;
        }
        let fixture = Fixture::new();
        let first = fixture.source_with_chord(shell, "'cj'", "ctrl-o");
        let second = fixture.source_with_chord(shell, "'cj'", "alt-o");
        let disabled = fixture.source_with_chord(shell, "", "alt-o");
        let query = if shell == "bash" {
            "bind -X 2>/dev/null"
        } else {
            "bindkey '^O'; bindkey '^[o'"
        };
        let replace = if shell == "bash" {
            r#"bind '"\C-o":beginning-of-line'"#
        } else {
            "bindkey '^O' beginning-of-line"
        };
        let query_user = if shell == "bash" {
            "bind -q beginning-of-line"
        } else {
            "bindkey '^O'"
        };
        let script = fixture.temp.path().join("reload.sh");
        fs::write(&script, format!("{first}\n{second}\n{query}\nprintf '\\0'\n{disabled}\n{query}\nprintf '\\0'\n{first}\n{replace}\n{disabled}\n{query_user}\n")).unwrap();
        let output = Command::new(executable).arg(&script).output().unwrap();
        assert_success(&output);
        let sections = fields(&output);
        assert!(
            sections[0].contains("_cj_key_widget"),
            "{shell}: {}",
            sections[0]
        );
        if shell == "bash" {
            assert!(!sections[0].contains("\\C-o"));
            assert!(sections[0].contains("\\eo"));
        } else {
            assert!(sections[0].contains("\"^O\" undefined-key"));
        }
        assert!(
            !sections[1].contains("_cj_key_widget"),
            "{shell}: {}",
            sections[1]
        );
        assert!(sections[2].contains("beginning-of-line"));
        if shell == "bash" {
            assert!(sections[2].contains("\\C-o"));
        }
    }
}
