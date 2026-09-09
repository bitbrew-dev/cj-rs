#![cfg(unix)]

mod support;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use support::{TempDir, assert_success, cj, write_executable};

#[test]
fn real_psreadline_keypresses_preserve_buffers_history_and_cwd() {
    let pwsh = env::var_os("CJ_TEST_PWSH").unwrap_or_else(|| "pwsh".into());
    if Command::new(&pwsh).arg("-Version").output().is_err()
        || Command::new("zsh").arg("--version").output().is_err()
    {
        assert!(env::var_os("CJ_REQUIRE_SHELLS").is_none());
        return;
    }
    for (behaviors, picker_status, candidates, snapshots) in [
        ("'zoxide', 'cj'", "0", "/candidate", 1),
        ("'cj', 'zoxide'", "0", "/candidate", 3),
        ("'zoxide', 'cj'", "130", "/candidate", 1),
        ("'zoxide', 'cj'", "7", "/candidate", 1),
        ("'zoxide', 'cj'", "0", "", 3),
    ] {
        let temp = TempDir::new("psreadline-雪");
        let zoxide = temp.path().join("custom zoxide's executable");
        let fzf = temp.path().join("custom fzf's executable");
        write_executable(
            &zoxide,
            r#"
case "$*" in
  'query --list') printf '%s' "$CJ_TEST_CANDIDATES" ;;
  'query --interactive') printf 'called\n' >> "$CJ_TEST_CALLS"; fzf ;;
  *) exit 2 ;;
esac
"#,
        );
        write_executable(
            &fzf,
            r#"
[ -t 0 ] || { echo 'picker requires a foreground terminal' >&2; exit 2; }
[ -t 2 ] || { echo 'picker stderr requires a foreground terminal' >&2; exit 2; }
printf '%s\n' "$CJ_TEST_PICK"
if [ "$CJ_TEST_PICK_STATUS" = 7 ]; then echo 'picker exploded' >&2; fi
exit "$CJ_TEST_PICK_STATUS"
"#,
        );
        let config = temp.path().join("config.toml");
        fs::write(&config, format!(
            "[programs]\nzoxide = {}\nfzf = {}\n[key-bindings]\nmacos = {{ key = 'ctrl-o', behaviors = [{behaviors}] }}\nlinux = {{ key = 'ctrl-o', behaviors = [{behaviors}] }}\n",
            serde_json::to_string(zoxide.to_str().unwrap()).unwrap(),
            serde_json::to_string(fzf.to_str().unwrap()).unwrap(),
        )).unwrap();
        let generated = cj(temp.path(), temp.path())
            .arg("-C")
            .arg(&config)
            .args(["init", "powershell", "--setup-key-binding"])
            .output()
            .unwrap();
        assert_success(&generated);
        let source = temp.path().join("editor.ps1");
        let mut script = String::from(
            "Import-Module PSReadLine\n$ErrorActionPreference = 'Stop'\nSet-PSReadLineOption -EditMode Emacs -PredictionSource None -HistorySaveStyle SaveNothing\n",
        );
        script.push_str(std::str::from_utf8(&generated.stdout).unwrap());
        script.push_str(r#"
$global:__cj_history = @($env:CJ_TEST_OLDER, $env:CJ_TEST_NEWER)
$global:__cj_down_route = '/unchanged/down/route'
$global:__cj_test_records = 0
# Only this independent observation chord is test-owned. Ctrl+o stays bound to
# the generated handler and is delivered through the terminal event loop.
Set-PSReadLineKeyHandler -Chord 'Ctrl+g' -ScriptBlock {
    $buffer = ''; $cursor = 0
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$buffer, [ref]$cursor)
    $record = @{ Buffer = $buffer; Cursor = $cursor; Cwd = (Get-Location).Path; History = @($global:__cj_history); Down = $global:__cj_down_route }
    [IO.File]::AppendAllText($env:CJ_TEST_RESULTS, ($record | ConvertTo-Json -Compress) + "`n")
    $global:__cj_test_records++
    if ($global:__cj_test_records -eq [int]$env:CJ_TEST_SNAPSHOTS) {
        [Microsoft.PowerShell.PSConsoleReadLine]::RevertLine()
        [Microsoft.PowerShell.PSConsoleReadLine]::Insert('exit')
        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
    }
}
function global:prompt {
    [IO.File]::WriteAllText($env:CJ_TEST_READY, 'ready')
    'cj-test> '
}
"#);
        fs::write(&source, script).unwrap();
        let launcher = temp.path().join("launch-pwsh");
        write_executable(
            &launcher,
            r#"
stty rows 40 cols 200
exec "$CJ_TEST_PWSH" -NoLogo -NoProfile -NoExit -File "$CJ_TEST_SCRIPT"
"#,
        );
        let results = temp.path().join("results.jsonl");
        let older = temp.path().join("older 'quoted' 雪");
        let newer = temp.path().join("-newer folder");
        let target = "/selected 'quote' 雪/‘’/$literal;$(no-execution)\nlast\n\n";
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_cj"));
        let mut paths = vec![binary.parent().unwrap().to_path_buf()];
        paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
        let output = Command::new("zsh")
            .args([
                "-fc",
                r#"
zmodload zsh/zpty || exit 1
zpty -b cj_editor "$CJ_TEST_LAUNCHER" || exit 1
sent=0
for attempt in {1..400}; do
    while zpty -r cj_editor output; do
        print -rn -- "$output"
        # .NET Console requests a cursor report while PSReadLine initializes.
        [[ "$output" == *$'\e[6n'* ]] && zpty -w -n cj_editor $'\e[1;1R'
    done
    if [[ -f "$CJ_TEST_READY" && $sent == 0 ]]; then
        sleep 0.1
        zpty -w -n cj_editor 'code '
        for count in {1..$CJ_TEST_SNAPSHOTS}; do zpty -w -n cj_editor $'\x0f\x07'; done
        sent=1
    fi
    zpty -t cj_editor || break
    sleep 0.025
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
            .env("TERM", "xterm-256color")
            .env("CJ_TEST_PWSH", &pwsh)
            .env("CJ_TEST_SCRIPT", &source)
            .env("CJ_TEST_LAUNCHER", &launcher)
            .env("CJ_TEST_RESULTS", &results)
            .env("CJ_TEST_READY", temp.path().join("ready"))
            .env("CJ_TEST_CALLS", temp.path().join("calls"))
            .env("CJ_TEST_SNAPSHOTS", snapshots.to_string())
            .env("CJ_TEST_OLDER", &older)
            .env("CJ_TEST_NEWER", &newer)
            .env("CJ_TEST_PICK", target)
            .env("CJ_TEST_PICK_STATUS", picker_status)
            .env("CJ_TEST_CANDIDATES", candidates)
            .output()
            .unwrap();
        assert_success(&output);
        let records: Vec<serde_json::Value> = fs::read_to_string(&results)
            .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)))
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), snapshots);
        for (index, record) in records.iter().enumerate() {
            let chosen = if snapshots == 3 {
                match index {
                    0 => Some(newer.to_str().unwrap()),
                    1 => Some(older.to_str().unwrap()),
                    _ => None,
                }
            } else if picker_status == "0" {
                Some(target)
            } else {
                None
            };
            let expected = chosen.map_or_else(
                || "code ".into(),
                |path| {
                    format!(
                        "code '{}'",
                        path.replace('\'', "''")
                            .replace('‘', "‘‘")
                            .replace('’', "’’")
                    )
                },
            );
            assert_eq!(record["Buffer"], expected, "{behaviors}, {picker_status}");
            assert_eq!(record["Cursor"], expected.encode_utf16().count());
            assert_eq!(record["Cwd"], temp.path().to_str().unwrap());
            assert_eq!(
                record["History"],
                serde_json::json!([older.to_str().unwrap(), newer.to_str().unwrap()])
            );
            assert_eq!(record["Down"], "/unchanged/down/route");
        }
        assert_eq!(temp.path().join("calls").exists(), snapshots == 1);
        if picker_status == "7" {
            assert!(String::from_utf8_lossy(&output.stdout).contains("picker exploded"));
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("interactive zoxide query failed"),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}
