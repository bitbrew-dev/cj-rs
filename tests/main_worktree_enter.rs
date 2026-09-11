mod support;

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use support::{GitFixture, assert_success, cj};

fn available(shell: &str) -> bool {
    if cfg!(windows) && matches!(shell, "bash" | "zsh") {
        return false;
    }
    let found = Command::new(shell)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    assert!(
        found
            || (env::var_os("CJ_REQUIRE_SHELLS").is_none()
                && !(shell == "pwsh" && env::var_os("CJ_REQUIRE_POWERSHELL").is_some())),
        "required shell unavailable: {shell}"
    );
    found
}

fn run(fixture: &GitFixture, shell: &str, config: &Path, cwd: &Path, body: &str) {
    let source = cj(cwd, fixture.temp.path())
        .arg("-C")
        .arg(config)
        .args(["init", shell, "--no-setup-key-binding"])
        .output()
        .unwrap();
    assert_success(&source);
    let file = fixture.temp.path().join(match shell {
        "nu" => "init.nu",
        "pwsh" => "init.ps1",
        _ => "init.sh",
    });
    fs::write(&file, source.stdout).unwrap();
    let mut paths = vec![
        Path::new(env!("CARGO_BIN_EXE_cj"))
            .parent()
            .unwrap()
            .to_path_buf(),
    ];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let mut command = Command::new(shell);
    match shell {
        "bash" | "zsh" => {
            command.args(if shell == "bash" {
                &["--noprofile", "--norc", "-c"][..]
            } else {
                &["-f", "-c"][..]
            });
            command.arg(format!("set -e; . \"$CJ_SOURCE\"; {body}"));
        }
        "nu" => {
            command
                .args(["--no-config-file", "-c"])
                .arg(format!("use r###'{}'### *; {body}", file.display()));
        }
        _ => {
            command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]).arg(format!(r#"$ErrorActionPreference = 'Stop'; . $env:CJ_SOURCE
function Same-Path($a, $b) {{
    $comparison = if ($env:OS -eq 'Windows_NT') {{ [StringComparison]::OrdinalIgnoreCase }} else {{ [StringComparison]::Ordinal }}
    [IO.Path]::GetFullPath($a).Equals([IO.Path]::GetFullPath($b), $comparison)
}}
{body}"#));
        }
    }
    let output = command
        .current_dir(cwd)
        .env("PATH", env::join_paths(paths).unwrap())
        .env("CJ_SOURCE", file)
        .env("CJ_MAIN", &fixture.main)
        .env("CJ_START", cwd)
        .env("CJ_COUNT", if cwd == fixture.main { "0" } else { "1" })
        .env("CJ_OUTSIDE", fixture.temp.path().join("outside"))
        .env("GIT_CEILING_DIRECTORIES", fixture.temp.path())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert_success(&output);
}

#[test]
fn bare_worktree_enter_uses_primary_checkout_and_records_only_real_moves() {
    let fixture = GitFixture::new("main-enter-‘’“”");
    let output = Command::new("git")
        .args(["branch", "-m", "production"])
        .current_dir(&fixture.main)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert_success(&output);
    for cwd in [&fixture.linked_nested, &fixture.main_nested, &fixture.main] {
        for shadow in ["origin", "og", "homebase"] {
            fs::create_dir_all(cwd.join(shadow)).unwrap();
        }
    }
    fs::create_dir(fixture.temp.path().join("outside")).unwrap();
    let config = fixture.temp.path().join("enter.toml");
    let mut exercised = 0;
    for keywords in ["[]", "[\"homebase\"]"] {
        fs::write(&config, format!("[keywords]\nmain-worktree = {keywords}\n[programs]\nfzf = \"missing-main-enter-fzf\"\nzoxide = \"missing-main-enter-zoxide\"\n")).unwrap();
        for shell in ["bash", "zsh", "nu", "pwsh"] {
            if !available(shell) {
                continue;
            }
            exercised += 1;
            let success = match shell {
                "bash" | "zsh" => {
                    r#"
for flag in -jw --jump-worktree; do
    cd "$flag"
    [[ $PWD == "$CJ_MAIN" && ${#_cj_history[@]} == "$CJ_COUNT" ]]
    if (( CJ_COUNT )); then cd v; fi
    [[ $PWD == "$CJ_START" && ${#_cj_history[@]} == 0 ]]
    cd "$flag" "$CJ_OUTSIDE"
    [[ $PWD == "$CJ_OUTSIDE" && ${#_cj_history[@]} == 1 ]]
    if cd "$flag" '' 2>/dev/null; then exit 1; else [[ $? == 2 ]]; fi
    if cd "$flag" "$CJ_MAIN" extra 2>/dev/null; then exit 1; else [[ $? == 2 ]]; fi
    [[ $PWD == "$CJ_OUTSIDE" && ${#_cj_history[@]} == 1 ]]
    cd v; [[ $PWD == "$CJ_START" && ${#_cj_history[@]} == 0 ]]
done"#
                }
                "nu" => {
                    r#"
for flag in [-jw --jump-worktree] {
    __cj_cd $flag
    if ($env.PWD | path expand) != ($env.CJ_MAIN | path expand) or ($env.__cj_history | length) != ($env.CJ_COUNT | into int) { error make {msg: 'wrong primary destination or history'} }
    if ($env.CJ_COUNT | into int) > 0 { __cj_cd v }
    if ($env.PWD | path expand) != ($env.CJ_START | path expand) or ($env.__cj_history | length) != 0 { error make {msg: 'back did not restore start'} }
}"#
                }
                _ => {
                    r#"
foreach ($flag in @('-jw', '--jump-worktree')) {
    cd $flag
    if (-not (Same-Path (Get-Location).ProviderPath $env:CJ_MAIN) -or $global:__cj_history.Count -ne [int]$env:CJ_COUNT) { throw 'wrong primary destination or history' }
    if ([int]$env:CJ_COUNT -gt 0) { cd v }
    if (-not (Same-Path (Get-Location).ProviderPath $env:CJ_START) -or $global:__cj_history.Count -ne 0) { throw 'back did not restore start' }
}"#
                }
            };
            for cwd in [&fixture.linked_nested, &fixture.main_nested, &fixture.main] {
                run(&fixture, shell, &config, cwd, success);
            }
            let failure = match shell {
                "bash" | "zsh" => {
                    r#"
cd "$CJ_OUTSIDE"
for flag in -jw --jump-worktree; do
    if cd "$flag" 2>/dev/null; then exit 1; else [[ $? == 2 ]]; fi
    [[ $PWD == "$CJ_OUTSIDE" && ${#_cj_history[@]} == 1 ]]
done
cd v; [[ $PWD == "$CJ_START" && ${#_cj_history[@]} == 0 ]]"#
                }
                "nu" => {
                    r#"
__cj_cd $env.CJ_OUTSIDE
for flag in [-jw --jump-worktree] {
    let failed = (try { __cj_cd $flag; false } catch { true })
    if not $failed or ($env.PWD | path expand) != ($env.CJ_OUTSIDE | path expand) or ($env.__cj_history | length) != 1 { error make {msg: 'nonrepo failure changed state'} }
}
__cj_cd v
if ($env.PWD | path expand) != ($env.CJ_START | path expand) or ($env.__cj_history | length) != 0 { error make {msg: 'failed command changed history target'} }"#
                }
                _ => {
                    r#"
cd $env:CJ_OUTSIDE
foreach ($flag in @('-jw', '--jump-worktree')) {
    $failed = $false
    try { cd $flag } catch { $failed = $true }
    if (-not $failed -or -not (Same-Path (Get-Location).ProviderPath $env:CJ_OUTSIDE) -or $global:__cj_history.Count -ne 1) { throw 'nonrepo failure changed state' }
}
cd v
if (-not (Same-Path (Get-Location).ProviderPath $env:CJ_START) -or $global:__cj_history.Count -ne 0) { throw 'failed command changed history target' }"#
                }
            };
            run(&fixture, shell, &config, fixture.temp.path(), failure);
        }
    }
    assert!(exercised > 0, "no supported shell available");
}
