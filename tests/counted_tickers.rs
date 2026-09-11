mod support;

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use support::{TempDir, assert_success, cj};

fn shells() -> Vec<(&'static str, &'static str)> {
    let mut candidates = vec![
        ("bash", "bash"),
        ("zsh", "zsh"),
        ("nu", "nu"),
        ("pwsh", "pwsh"),
    ];
    if cfg!(target_os = "macos") {
        candidates.push(("/bin/bash", "bash"));
    }
    candidates.retain(|(executable, shell)| {
        if cfg!(windows) && matches!(*shell, "bash" | "zsh") {
            return false;
        }
        let available = Command::new(executable)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        assert!(
            available
                || (env::var_os("CJ_REQUIRE_SHELLS").is_none()
                    && !(*shell == "pwsh" && env::var_os("CJ_REQUIRE_POWERSHELL").is_some())),
            "required shell unavailable: {executable}"
        );
        available
    });
    assert!(!candidates.is_empty(), "no supported shell available");
    candidates
}

fn run(temp: &TempDir, executable: &str, shell: &str, config: &Path, body: &str) -> String {
    let source = cj(temp.path(), temp.path())
        .arg("-C")
        .arg(config)
        .args(["init", shell, "--no-setup-key-binding"])
        .output()
        .unwrap();
    assert_success(&source);
    let file = temp.path().join(match shell {
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
    let mut command = Command::new(executable);
    match shell {
        "bash" | "zsh" => {
            command.args(if shell == "bash" {
                &["--noprofile", "--norc", "-c"][..]
            } else {
                &["-f", "-c"][..]
            });
            command.arg(format!(
                r#"set -e; . "$CJ_SOURCE"
snapshot() {{ printf '%s\n' "$PWD" "${{#_cj_history[@]}}"; }}
{body}"#
            ));
        }
        "nu" => {
            command.args(["--no-config-file", "-c"]).arg(format!(
                "use r###'{}'### *; hide cd; def snapshot [] {{ print $env.PWD; print ($env.__cj_history | length) }}; {body}", file.display()));
        }
        _ => {
            command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]).arg(format!(
                "$ErrorActionPreference = 'Stop'; . $env:CJ_SOURCE; function snapshot {{ (Get-Location).ProviderPath; $global:__cj_history.Count }}; {body}"));
        }
    }
    let output = command
        .current_dir(temp.path().join("a/b/c"))
        .env("PATH", env::join_paths(paths).unwrap())
        .env("CJ_SOURCE", file)
        .env("CJ_ROOT", temp.path())
        .env("HOME", temp.path().join("home"))
        .env("USERPROFILE", temp.path().join("home"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{executable}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

const POSIX: &str = r#"
for token in '@U@@U@' '@U@2' '@U@0002'; do
    cd "$token"; snapshot; cd '@D@2'; snapshot
done
cd -Z '@U@2'; snapshot; cd -Z '@D@2'; snapshot
if cd -r '@U@2' 2>/dev/null; then exit 1; fi
cd '@U@3'; cd '@D@@D@@D@'; snapshot
cd '@U@2147483647'; snapshot
cd '@U@2147483647'; snapshot
if cd '@D@2147483647' 2>/dev/null; then exit 1; fi
snapshot; cd '@D@@DEPTH@'; snapshot
cd '@U@1'
for token in '@U@0' '@U@-1' '@U@1.5' '@U@+1' '@U@1e2' '@U@2147483648' '@U@99999999999999999999999' '@D@0' '@D@-1' '@D@1.5' '@D@+1' '@D@1e2' '@D@2147483648' '@D@99999999999999999999999' '@D@2' $'@U@1\n' $'@U@1\r' $'@D@1\n' $'@D@1\r'; do
    before_pwd=$PWD; before_history="${_cj_history[*]}"
    if cd "$token" 2>/dev/null; then printf 'accepted invalid token: %s\n' "$token" >&2; exit 1; fi
    [[ $PWD == "$before_pwd" && ${_cj_history[*]} == "$before_history" ]]
done
cd '@D@1'; snapshot
for token in '@U@2' '@U@-1' '@D@3' '@D@0'; do
    mkdir "$token"; cd "$token"; snapshot; cd '@D@1'
done
snapshot
"#;

const NU: &str = r#"
for token in ['@U@@U@' '@U@2' '@U@0002'] {
    __cj_cd $token; snapshot; __cj_cd '@D@2'; snapshot
}
__cj_cd -Z '@U@2'; snapshot; __cj_cd -Z '@D@2'; snapshot
let raw_failed = (try { __cj_cd -r '@U@2'; false } catch { true })
if not $raw_failed { error make {msg: 'raw path interpreted as ticker'} }
__cj_cd '@U@3'; __cj_cd '@D@@D@@D@'; snapshot
__cj_cd '@U@2147483647'; snapshot
__cj_cd '@U@2147483647'; snapshot
let exceeded = (try { __cj_cd '@D@2147483647'; false } catch { true })
if not $exceeded { error make {msg: 'accepted unavailable history'} }
snapshot; __cj_cd '@D@@DEPTH@'; snapshot
__cj_cd '@U@1'
for token in ['@U@0' '@U@-1' '@U@1.5' '@U@+1' '@U@1e2' '@U@2147483648' '@U@99999999999999999999999' '@D@0' '@D@-1' '@D@1.5' '@D@+1' '@D@1e2' '@D@2147483648' '@D@99999999999999999999999' '@D@2' "@U@1\n" "@U@1\r" "@D@1\n" "@D@1\r"] {
    let before_pwd = $env.PWD; let before_history = $env.__cj_history
    let failed = (try { __cj_cd $token; false } catch { true })
    if not $failed or $env.PWD != $before_pwd or $env.__cj_history != $before_history { error make {msg: $'invalid token changed state: ($token)'} }
}
__cj_cd '@D@1'; snapshot
for token in ['@U@2' '@U@-1' '@D@3' '@D@0'] {
    mkdir $token; __cj_cd $token; snapshot; __cj_cd '@D@1'
}
snapshot
"#;

const POWERSHELL: &str = r#"
foreach ($token in @('@U@@U@', '@U@2', '@U@0002')) {
    cd $token; snapshot; cd '@D@2'; snapshot
}
cd -Z '@U@2'; snapshot; cd -Z '@D@2'; snapshot
$rawFailed = $false; try { cd -r '@U@2' } catch { $rawFailed = $true }
if (-not $rawFailed) { throw 'raw path interpreted as ticker' }
cd '@U@3'; cd '@D@@D@@D@'; snapshot
cd '@U@2147483647'; snapshot
cd '@U@2147483647'; snapshot
$exceeded = $false
try { cd '@D@2147483647' } catch { $exceeded = $true }
if (-not $exceeded) { throw 'accepted unavailable history' }
snapshot; cd '@D@@DEPTH@'; snapshot
cd '@U@1'
foreach ($token in @('@U@0', '@U@-1', '@U@1.5', '@U@+1', '@U@1e2', '@U@2147483648', '@U@99999999999999999999999', '@D@0', '@D@-1', '@D@1.5', '@D@+1', '@D@1e2', '@D@2147483648', '@D@99999999999999999999999', '@D@2', "@U@1`n", "@U@1`r", "@D@1`n", "@D@1`r")) {
    $beforePwd = (Get-Location).ProviderPath; $beforeHistory = $global:__cj_history | ConvertTo-Json -Compress
    $failed = $false; try { cd $token } catch { $failed = $true }
    if (-not $failed -or (Get-Location).ProviderPath -ne $beforePwd -or ($global:__cj_history | ConvertTo-Json -Compress) -ne $beforeHistory) { throw "invalid token changed state: $token" }
}
cd '@D@1'; snapshot
foreach ($token in @('@U@2', '@U@-1', '@D@3', '@D@0')) {
    [IO.Directory]::CreateDirectory((Join-Path (Get-Location).ProviderPath $token)) > $null
    cd $token; snapshot; cd '@D@1'
}
snapshot
"#;

#[test]
fn counted_tickers_match_repetition_and_preserve_history_boundaries_in_all_shells() {
    for (up, down) in [("^", "v"), ("u", "k")] {
        for (executable, shell) in shells() {
            let temp = TempDir::new("counted-tickers ' spaces");
            let start = temp.path().join("a/b/c");
            fs::create_dir_all(&start).unwrap();
            fs::create_dir(temp.path().join("home")).unwrap();
            let config = temp.path().join("config.toml");
            fs::write(&config, format!("[behavior]\ndefault = \"builtin\"\n[tickers]\nnavigate_up = \"{up}\"\nnavigate_down = \"{down}\"\n")).unwrap();
            let depth = start.ancestors().count() - 1;
            let root = start.ancestors().last().unwrap();
            let script = match shell {
                "nu" => NU,
                "pwsh" => POWERSHELL,
                _ => POSIX,
            }
            .replace("@U@", up)
            .replace("@D@", down)
            .replace("@DEPTH@", &depth.to_string());
            let output = run(&temp, executable, shell, &config, &script);
            let mut expected = Vec::new();
            for _ in 0..4 {
                expected.push((temp.path().join("a"), 2));
                expected.push((start.clone(), 0));
            }
            expected.push((start.clone(), 0));
            for _ in 0..3 {
                expected.push((root.to_path_buf(), depth));
            }
            expected.push((start.clone(), 0));
            expected.push((start.clone(), 0));
            for token in [
                format!("{up}2"),
                format!("{up}-1"),
                format!("{down}3"),
                format!("{down}0"),
            ] {
                expected.push((start.join(token), 1));
            }
            expected.push((start.clone(), 0));
            let lines: Vec<_> = output.lines().collect();
            assert_eq!(lines.len(), expected.len() * 2, "{executable}: {output}");
            for (actual, (path, count)) in lines.chunks_exact(2).zip(expected) {
                // Shells can spell Windows separators differently; compare actual paths.
                assert_eq!(
                    dunce::canonicalize(actual[0]).unwrap(),
                    dunce::canonicalize(path).unwrap(),
                    "{executable}"
                );
                assert_eq!(actual[1], count.to_string(), "{executable}");
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn counted_parent_history_keeps_logical_symlink_steps() {
    for (executable, shell) in shells()
        .into_iter()
        .filter(|(_, shell)| matches!(*shell, "bash" | "zsh"))
    {
        let temp = TempDir::new("counted-symlink");
        let physical = temp.path().join("physical/deep/leaf");
        fs::create_dir_all(&physical).unwrap();
        fs::create_dir_all(temp.path().join("a/b/c")).unwrap();
        std::os::unix::fs::symlink(&physical, temp.path().join("a/link")).unwrap();
        let deep = format!("a/b/c/{}link", "d/".repeat(12));
        fs::create_dir(temp.path().join("shallow")).unwrap();
        let deep_link = temp.path().join(&deep);
        fs::create_dir_all(deep_link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(temp.path().join("shallow"), deep_link).unwrap();
        let config = temp.path().join("config.toml");
        fs::write(&config, "[behavior]\ndefault = \"builtin\"\n").unwrap();
        let output = run(
            &temp,
            executable,
            shell,
            &config,
            &r#"
builtin cd "$CJ_ROOT/a/link"
cd '^2'
[[ $PWD == "$CJ_ROOT" && ${#_cj_history[@]} == 2 ]]
cd v1
[[ $PWD == "$CJ_ROOT/a" && ${#_cj_history[@]} == 1 ]]
cd v1
[[ $PWD == "$CJ_ROOT/physical/deep/leaf" && ${#_cj_history[@]} == 0 ]]
builtin cd "$CJ_ROOT/@DEEP@"
if [[ -n ${BASH_VERSION-} ]]; then export -n PWD; else typeset +x PWD; fi
cd '^13'
[[ $PWD == "$CJ_ROOT/a/b/c" && ${#_cj_history[@]} == 13 ]]
cd v13
[[ $PWD == "$CJ_ROOT/shallow" && ${#_cj_history[@]} == 0 ]]
"#
            .replace("@DEEP@", &deep),
        );
        assert!(output.is_empty(), "{executable}: {output}");
    }
}
