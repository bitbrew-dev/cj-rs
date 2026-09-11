mod support;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use support::{GitFixture, assert_success, cj, write_config};

fn available(shell: &str) -> bool {
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
    available
}

fn integration(fixture: &GitFixture, shell: &str) -> PathBuf {
    let config = fixture.temp.path().join("completion config.toml");
    write_config(
        &config,
        Path::new("missing-worktree-test-zoxide"),
        Path::new("missing-worktree-test-fzf"),
    );
    let output = cj(&fixture.main_nested, fixture.temp.path())
        .arg("-C")
        .arg(config)
        .args(["init", shell, "--no-setup-key-binding"])
        .output()
        .unwrap();
    assert_success(&output);
    let path = fixture.temp.path().join(if shell == "nu" {
        "completion.nu"
    } else {
        "completion.ps1"
    });
    fs::write(&path, output.stdout).unwrap();
    path
}

fn command(fixture: &GitFixture, shell: &str) -> Command {
    let binary = Path::new(env!("CARGO_BIN_EXE_cj"));
    let mut paths = vec![binary.parent().unwrap().to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let mut command = Command::new(shell);
    command
        .current_dir(&fixture.main_nested)
        .env("PATH", env::join_paths(paths).unwrap())
        .env("CJ_EXPECTED", &fixture.linked)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE");
    command
}

#[test]
fn nushell_worktree_completion_after_spaces_executes_and_records_history() {
    if !available("nu") {
        return;
    }
    let fixture = GitFixture::new("nu-spaced-worktree-雪");
    let source = integration(&fixture, "nu");
    let prelude = format!("use r#'{}'# *; ", source.display());
    for flag in ["-jw", "--jump-worktree"] {
        for space in ["", " ", "  "] {
            let line = format!("cd {flag}{space}");
            let script = format!(
                "{prelude}let before = $env.PWD; let matches = ($env.CJ_LINE | commandline complete --detailed); if $env.PWD != $before or ($env.__cj_history | length) != 0 {{ error make {{msg: 'completion changed navigation state'}} }}; $matches | to json -r"
            );
            let output = command(&fixture, "nu")
                .args(["--no-config-file", "-c", &script])
                .env("CJ_LINE", &line)
                .output()
                .unwrap();
            assert_success(&output);
            let matches: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(matches.len(), 2, "{line}: {matches:?}");
            let selected = matches
                .iter()
                .find(|entry| entry["value"].as_str().unwrap().contains("feature"))
                .unwrap();
            let start = selected["span"]["start"].as_u64().unwrap() as usize;
            let end = selected["span"]["end"].as_u64().unwrap() as usize;
            let completed = format!(
                "{}{}{}",
                &line[..start],
                selected["value"].as_str().unwrap(),
                &line[end..]
            );
            let script = format!(
                "{prelude}let before = $env.PWD; {completed}; if ($env.PWD | path expand) != ($env.CJ_EXPECTED | path expand) or ($env.__cj_history | length) != 1 {{ error make {{msg: 'selected completion did not navigate and record history'}} }}; cd v; if $env.PWD != $before or ($env.__cj_history | length) != 0 {{ error make {{msg: 'back did not restore original directory'}} }}"
            );
            let output = command(&fixture, "nu")
                .args(["--no-config-file", "-c", &script])
                .output()
                .unwrap();
            assert_success(&output);
        }
        let script = format!(
            "{prelude}let before = $env.PWD; let matches = ($env.CJ_LINE | commandline complete --detailed); if ($matches | length) != 1 {{ error make {{msg: 'partial worktree path did not filter candidates'}} }}; for args in [[$env.CJ_FLAG ''] [$env.CJ_FLAG 'missing-worktree'] [$env.CJ_FLAG $env.CJ_EXPECTED 'extra']] {{ let failed = (try {{ __cj_cd ...$args; false }} catch {{ true }}); if not $failed {{ error make {{msg: 'invalid worktree invocation accepted'}} }} }}; if $env.PWD != $before or ($env.__cj_history | length) != 0 {{ error make {{msg: 'failed navigation changed state'}} }}"
        );
        let output = command(&fixture, "nu")
            .args(["--no-config-file", "-c", &script])
            .env("CJ_FLAG", flag)
            .env(
                "CJ_LINE",
                format!("cd {flag} `{}`", fixture.linked.display()),
            )
            .output()
            .unwrap();
        assert_success(&output);
    }
}

#[test]
fn powershell_worktree_completion_replaces_flag_and_spaces() {
    if !available("pwsh") {
        return;
    }
    let fixture = GitFixture::new("pwsh-spaced-worktree-雪");
    let source = integration(&fixture, "pwsh");
    let script = r#"$ErrorActionPreference = 'Stop'
. $env:CJ_SOURCE
$before = (Get-Location).ProviderPath
foreach ($flag in @('-jw', '--jump-worktree')) {
    foreach ($space in @('', ' ', '  ', "`t")) {
        $line = "cd $flag$space"
        $completion = TabExpansion2 -inputScript $line -cursorColumn $line.Length
        if ($completion.CompletionMatches.Count -ne 2) { throw "wrong candidate count: $line" }
        if ($completion.ReplacementIndex -ne 3 -or $completion.ReplacementLength -ne ($line.Length - 3)) { throw "wrong replacement span: $line" }
        if ((Get-Location).ProviderPath -cne $before -or $global:__cj_history.Count -ne 0) { throw 'completion changed navigation state' }
        $selected = $completion.CompletionMatches | Where-Object { $_.ListItemText.Contains('feature') } | Select-Object -First 1
        $completed = $line.Remove($completion.ReplacementIndex, $completion.ReplacementLength).Insert($completion.ReplacementIndex, $selected.CompletionText)
        if (-not $completed.StartsWith("cd '")) { throw "jump flag remained in completed command: $completed" }
        # Test execution models pressing Enter on the shell-produced completion.
        Invoke-Expression $completed
        if (-not (Get-Location).ProviderPath.Equals($env:CJ_EXPECTED, [System.StringComparison]::OrdinalIgnoreCase) -or $global:__cj_history.Count -ne 1) { throw 'completion did not navigate and record history' }
        cd v
        if ((Get-Location).ProviderPath -cne $before -or $global:__cj_history.Count -ne 0) { throw 'back did not restore original directory' }
    }
    cd $flag $env:CJ_EXPECTED
    if ($global:__cj_history.Count -ne 1) { throw 'literal selected argument did not record history' }
    cd v
    foreach ($arguments in @(@($flag, ''), @($flag, 'missing-worktree'), @($flag, $env:CJ_EXPECTED, 'extra'))) {
        $failed = $false
        try { cd @arguments } catch { $failed = $true }
        if (-not $failed) { throw 'invalid worktree invocation accepted' }
    }
    if ((Get-Location).ProviderPath -cne $before -or $global:__cj_history.Count -ne 0) { throw 'failed navigation changed state' }
}
"#;
    let output = command(&fixture, "pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env("CJ_SOURCE", source)
        .output()
        .unwrap();
    assert_success(&output);
}

#[cfg(unix)]
#[test]
fn nushell_reedline_inserts_and_executes_quoted_worktree_paths() {
    if !available("nu") {
        return;
    }
    let temp = support::TempDir::new("nu-worktree-tab-pty");
    let output = Command::new("python3")
        .arg("-c")
        .arg(include_str!("support/nu_worktree_pty.py"))
        .arg(temp.path())
        .arg(env!("CARGO_BIN_EXE_cj"))
        .output()
        .unwrap();
    assert_success(&output);
}
