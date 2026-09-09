mod support;

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use support::{TempDir, assert_success, cj};

/// Exercise generated integrations in real shells, including CI's Windows PowerShell.
fn exercise(label: &str, posix: &str, nu: &str, pwsh: &str, expected: &[&str]) {
    let temp = TempDir::new(&format!("{label} ' spaced path"));
    for directory in ["a/b/c", "other", "home", "d"] {
        fs::create_dir_all(temp.path().join(directory)).unwrap();
    }
    let config = temp.path().join("history.toml");
    let alias = temp
        .path()
        .join("other")
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    fs::write(
        &config,
        format!("[behavior]\ndefault = \"builtin\"\n[aliases]\nelsewhere = \"{alias}\"\n[mounts]\nportable = {{ path = \"{alias}\" }}\n"),
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
    for shell in ["bash", "zsh", "nu", "pwsh"] {
        if cfg!(windows) && matches!(shell, "bash" | "zsh") {
            continue;
        }
        let available = Command::new(shell)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        assert!(
            available
                || (env::var_os("CJ_REQUIRE_SHELLS").is_none()
                    && !(shell == "pwsh" && env::var_os("CJ_REQUIRE_POWERSHELL").is_some())),
            "required shell unavailable: {shell}"
        );
        if !available {
            continue;
        }
        fs::create_dir_all(temp.path().join("other")).unwrap();
        let source = cj(temp.path(), temp.path())
            .arg("-C")
            .arg(&config)
            .args(["init", shell])
            .output()
            .unwrap();
        assert_success(&source);
        let file = temp.path().join(if shell == "nu" {
            "init.nu"
        } else if shell == "pwsh" {
            "init.ps1"
        } else {
            "init.sh"
        });
        fs::write(&file, source.stdout).unwrap();
        let mut command = Command::new(shell);
        match shell {
            "bash" | "zsh" => {
                command.args(if shell == "bash" {
                    &["--noprofile", "--norc", "-c"][..]
                } else {
                    &["-f", "-c"][..]
                });
                command.arg(format!("set -e; . \"$CJ_SOURCE\"; snapshot() {{ printf '%s\\n' \"$PWD\"; printf '%s\\n' \"${{#_cj_history[@]}}\"; }}; {posix}"));
            }
            "nu" => {
                let quoted = file.to_string_lossy();
                command.args(["--no-config-file", "-c"]).arg(format!("use r#'{quoted}'# *; hide cd; def snapshot [] {{ print $env.PWD; print ($env.__cj_history | length) }}; {nu}"));
            }
            _ => {
                command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]).arg(format!("$ErrorActionPreference = 'Stop'; . $env:CJ_SOURCE; function snapshot {{ (Get-Location).ProviderPath; $global:__cj_history.Count }}; {pwsh}"));
            }
        }
        let output = command
            .current_dir(temp.path().join("a/b/c"))
            .env("PATH", &path)
            .env("CJ_SOURCE", &file)
            .env("CJ_ROOT", temp.path())
            .env("HOME", temp.path().join("home"))
            .env("USERPROFILE", temp.path().join("home"))
            .output()
            .unwrap();
        assert_success(&output);
        let stdout = String::from_utf8(output.stdout).unwrap();
        let actual: Vec<_> = stdout.lines().collect();
        let expected: Vec<_> = expected
            .iter()
            .map(|line| {
                if let Some(relative) = line.strip_prefix("@/") {
                    if relative.is_empty() {
                        temp.path().to_string_lossy().into_owned()
                    } else {
                        relative
                            .split('/')
                            .fold(temp.path().to_path_buf(), |path, component| {
                                path.join(component)
                            })
                            .to_string_lossy()
                            .into_owned()
                    }
                } else {
                    line.to_string()
                }
            })
            .collect();
        assert_eq!(
            actual,
            expected,
            "{shell}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        exercised += 1;
    }
    assert!(exercised > 0, "no supported shell available");
}

#[test]
fn up_records_each_parent_and_back_consumes_without_push() {
    exercise(
        "history-parents",
        "cd '^^^'; snapshot; cd v; snapshot; cd vv; snapshot",
        "__cj_cd '^^^'; snapshot; __cj_cd v; snapshot; __cj_cd vv; snapshot",
        "cd '^^^'; snapshot; cd v; snapshot; cd vv; snapshot",
        &["@/", "3", "@/a", "2", "@/a/b/c", "0"],
    );
}

#[test]
fn direct_raw_alias_home_and_previous_directory_moves_are_recorded() {
    exercise(
        "history-all-moves",
        "cd \"$CJ_ROOT/other\"; cd -r \"$CJ_ROOT/d\"; cd elsewhere; cd >/dev/null; cd - >/dev/null; snapshot; cd vvv; snapshot; cd vv; snapshot",
        "__cj_cd ($env.CJ_ROOT | path join other); __cj_cd -r ($env.CJ_ROOT | path join d); __cj_cd elsewhere; __cj_cd; __cj_cd -; snapshot; __cj_cd vvv; snapshot; __cj_cd vv; snapshot",
        "cd (Join-Path $env:CJ_ROOT other); cd -r (Join-Path $env:CJ_ROOT d); cd elsewhere; cd; cd -; snapshot; cd vvv; snapshot; cd vv; snapshot",
        &["@/other", "5", "@/d", "2", "@/a/b/c", "0"],
    );
}

#[test]
fn failures_noops_and_native_escape_preserve_history() {
    exercise(
        "history-preservation",
        "cd \"$CJ_ROOT/other\"; cd .; cd -r .; cd missing 2>/dev/null && exit 1; cd vv 2>/dev/null && exit 1; builtin cd \"$CJ_ROOT/d\"; snapshot; cd v; snapshot",
        "__cj_cd ($env.CJ_ROOT | path join other); __cj_cd .; __cj_cd -r .; try { __cj_cd missing }; try { __cj_cd vv }; cd ($env.CJ_ROOT | path join d); snapshot; __cj_cd v; snapshot",
        "cd (Join-Path $env:CJ_ROOT other); cd .; cd -r .; try { cd missing } catch {}; try { cd vv } catch {}; Microsoft.PowerShell.Management\\Set-Location -LiteralPath (Join-Path $env:CJ_ROOT d); snapshot; cd v; snapshot",
        &["@/d", "1", "@/a/b/c", "0"],
    );
}

#[test]
fn failed_history_destination_is_atomic() {
    exercise(
        "history-stale",
        "cd \"$CJ_ROOT/other\"; cd \"$CJ_ROOT/d\"; rmdir \"$CJ_ROOT/other\"; cd v 2>/dev/null && exit 1; snapshot; cd vv; snapshot",
        "__cj_cd ($env.CJ_ROOT | path join other); __cj_cd ($env.CJ_ROOT | path join d); rm ($env.CJ_ROOT | path join other); try { __cj_cd v }; snapshot; __cj_cd vv; snapshot",
        "cd (Join-Path $env:CJ_ROOT other); cd (Join-Path $env:CJ_ROOT d); Remove-Item -LiteralPath (Join-Path $env:CJ_ROOT other); try { cd v } catch {}; snapshot; cd vv; snapshot",
        &["@/d", "2", "@/a/b/c", "0"],
    );
}

#[test]
fn stack_is_bounded_to_the_latest_hundred_departures() {
    exercise(
        "history-bound",
        "for i in {1..51}; do cd \"$CJ_ROOT/other\"; cd \"$CJ_ROOT/d\"; done; snapshot; printf '%s\\n' \"${_cj_history[@]:0:1}\"",
        "for i in 1..51 { __cj_cd ($env.CJ_ROOT | path join other); __cj_cd ($env.CJ_ROOT | path join d) }; snapshot; print ($env.__cj_history | first)",
        "foreach ($i in 1..51) { cd (Join-Path $env:CJ_ROOT other); cd (Join-Path $env:CJ_ROOT d) }; snapshot; $global:__cj_history[0]",
        &["@/d", "100", "@/d"],
    );
}

#[test]
fn history_noop_and_empty_history_do_not_change_directory_or_stack() {
    exercise(
        "history-noop",
        "cd v 2>/dev/null && exit 1; snapshot; cd \"$CJ_ROOT/other\"; builtin cd \"$CJ_ROOT/a/b/c\"; cd v; snapshot",
        "try { __cj_cd v }; snapshot; __cj_cd ($env.CJ_ROOT | path join other); cd ($env.CJ_ROOT | path join a b c); __cj_cd v; snapshot",
        "try { cd v } catch {}; snapshot; cd (Join-Path $env:CJ_ROOT other); Microsoft.PowerShell.Management\\Set-Location -LiteralPath (Join-Path $env:CJ_ROOT a/b/c); cd v; snapshot",
        &["@/a/b/c", "0", "@/a/b/c", "1"],
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn posix_history_preserves_non_utf8_directory_names() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let temp = TempDir::new("history-bytes");
    let directory = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"raw-\xff directory".to_vec()));
    fs::create_dir(&directory).unwrap();
    for shell in ["bash", "zsh"] {
        let source = cj(temp.path(), temp.path())
            .args(["init", shell])
            .output()
            .unwrap();
        assert_success(&source);
        let output = Command::new(shell)
            .args([
                "-c",
                "set -e; eval \"$1\"; cd -r \"$2\"; cd -r \"$3\"; cd v; printf '%s' \"$PWD\"",
                "_",
            ])
            .arg(String::from_utf8(source.stdout).unwrap())
            .arg(&directory)
            .arg(temp.path())
            .current_dir(temp.path())
            .output();
        let output = match output {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("{error}"),
        };
        assert_success(&output);
        assert_eq!(output.stdout, directory.as_os_str().as_bytes(), "{shell}");
    }
}

#[test]
fn mount_and_no_zoxide_paths_use_the_same_history() {
    exercise(
        "history-mount",
        "cd portable; snapshot; cd -Z \"$CJ_ROOT/d\"; snapshot; cd -Z vv; snapshot",
        "__cj_cd portable; snapshot; __cj_cd -Z ($env.CJ_ROOT | path join d); snapshot; __cj_cd -Z vv; snapshot",
        "cd portable; snapshot; cd -Z (Join-Path $env:CJ_ROOT d); snapshot; cd -Z vv; snapshot",
        &["@/other", "1", "@/d", "2", "@/a/b/c", "0"],
    );
}

#[cfg(unix)]
#[test]
fn parent_history_follows_native_logical_symlink_traversal() {
    let temp = TempDir::new("history-symlink");
    let physical = temp.path().join("physical/deep/leaf");
    let logical = temp.path().join("logical");
    fs::create_dir_all(&physical).unwrap();
    std::os::unix::fs::symlink(&physical, &logical).unwrap();
    let mut paths = vec![
        Path::new(env!("CARGO_BIN_EXE_cj"))
            .parent()
            .unwrap()
            .to_path_buf(),
    ];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(paths).unwrap();
    for shell in ["bash", "zsh", "nu"] {
        let source = cj(temp.path(), temp.path())
            .args(["init", shell])
            .output()
            .unwrap();
        assert_success(&source);
        let output = if shell == "nu" {
            let file = temp.path().join("init.nu");
            fs::write(&file, source.stdout).unwrap();
            let script = format!(
                "use r#'{}'# *; hide cd; cd r#'{}'#; __cj_cd '^'; if $env.PWD != r#'{}'# or ($env.__cj_history | length) != 1 {{ error make {{ msg: 'incorrect logical parent history' }} }}; __cj_cd v; print -n $env.PWD",
                file.display(),
                logical.display(),
                temp.path().display()
            );
            Command::new(shell)
                .args(["--no-config-file", "-c", &script])
                .env("PATH", &path)
                .output()
        } else {
            Command::new(shell).args(["-c", "set -e; eval \"$1\"; builtin cd \"$2\"; cd '^'; [[ $PWD == \"$3\" ]]; [[ ${#_cj_history[@]} == 1 ]]; cd v; printf '%s' \"$PWD\"", "_"]).arg(String::from_utf8(source.stdout).unwrap()).arg(&logical).arg(temp.path()).current_dir(temp.path()).env("PATH", &path).output()
        };
        let output = match output {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("{error}"),
        };
        assert_success(&output);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            physical.to_string_lossy(),
            "{shell}"
        );
    }
}
