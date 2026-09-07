#![cfg(windows)]

mod support;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use support::{GitFixture, TempDir, assert_success, cj, write_config};

#[test]
fn native_fake_zoxide_preserves_arguments_and_destination() {
    let fixture = ToolFixture::new("windows-zoxide");
    let output = fixture
        .zoxide_command()
        .args(["-z", "two words", "quo'te"])
        .output()
        .expect("run cj with fake zoxide");
    assert_success(&output);
    assert_eq!(output.stdout, path_output(&fixture.destination));
    assert_eq!(
        nul_strings(&fs::read(&fixture.args).expect("read arguments")),
        [
            "query",
            "--exclude",
            fixture.cwd.to_str().unwrap(),
            "--",
            "two words",
            "quo'te",
        ]
    );
}

#[test]
fn native_fake_fzf_uses_nul_records_and_returns_worktree() {
    let git = GitFixture::new("windows-fzf");
    let tool = copy_tool(git.temp.path(), "fzf.exe");
    let config = git.temp.path().join("fzf config.toml");
    let args = git.temp.path().join("fzf args");
    let stdin = git.temp.path().join("fzf stdin");
    write_config(&config, Path::new("missing-zoxide.exe"), &tool);

    let output = cj(&git.main, git.temp.path())
        .arg("-C")
        .arg(&config)
        .arg("--jump-worktree")
        .env("CJ_FAKE_TOOL", "fzf")
        .env("CJ_FAKE_MODE", "success")
        .env("CJ_FAKE_SELECTION", "1")
        .env("CJ_FAKE_ARGS", &args)
        .env("CJ_FAKE_STDIN", &stdin)
        .output()
        .expect("run cj picker");
    assert_success(&output);
    let selected = String::from_utf8(output.stdout).expect("selected path is UTF-8");
    assert_eq!(
        selected.trim_end().replace('\\', "/"),
        git.linked.to_string_lossy().replace('\\', "/")
    );
    let input = fs::read(stdin).expect("read fzf stdin");
    assert_eq!(input.iter().filter(|byte| **byte == 0).count(), 2);
    assert!(nul_strings(&fs::read(args).unwrap()).contains(&"--read0"));
}

#[test]
fn powershell_wrapper_changes_its_calling_session_with_zoxide() {
    let fixture = ToolFixture::new("windows-powershell-zoxide");
    let source = fixture.temp.path().join("cj.ps1");
    let init = cj(&fixture.cwd, fixture.temp.path())
        .arg("-C")
        .arg(&fixture.config)
        .args(["init", "powershell"])
        .output()
        .expect("generate PowerShell integration");
    assert_success(&init);
    fs::write(&source, init.stdout).expect("write PowerShell integration");

    let output = Command::new("pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            ". $env:CJ_SOURCE; cd -z 'two words'; [Console]::Out.Write((Get-Location).ProviderPath)",
        ])
        .current_dir(&fixture.cwd)
        .env("CJ_SOURCE", source)
        .env("CJ_FAKE_TOOL", "zoxide")
        .env("CJ_FAKE_MODE", "success")
        .env("CJ_FAKE_DEST", &fixture.destination)
        .env("CJ_FAKE_ARGS", &fixture.args)
        .env("PATH", path_with_cj())
        .output()
        .expect("run PowerShell integration");
    assert_success(&output);
    assert_eq!(
        output.stdout,
        fixture.destination.as_os_str().as_encoded_bytes()
    );
}

#[test]
fn powershell_wrapper_jumps_to_the_selected_worktree() {
    let git = GitFixture::new("windows-powershell-fzf");
    let tool = copy_tool(git.temp.path(), "fzf.exe");
    let config = git.temp.path().join("fzf config.toml");
    let source = git.temp.path().join("cj.ps1");
    let args = git.temp.path().join("fzf args");
    let stdin = git.temp.path().join("fzf stdin");
    write_config(&config, Path::new("missing-zoxide.exe"), &tool);
    let init = cj(&git.main_nested, git.temp.path())
        .arg("-C")
        .arg(&config)
        .args(["init", "powershell"])
        .output()
        .expect("generate PowerShell integration");
    assert_success(&init);
    fs::write(&source, init.stdout).expect("write PowerShell integration");

    let output = Command::new("pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            ". $env:CJ_SOURCE; cd -jw; [Console]::Out.Write((Get-Location).ProviderPath)",
        ])
        .current_dir(&git.main_nested)
        .env("CJ_SOURCE", source)
        .env("CJ_FAKE_TOOL", "fzf")
        .env("CJ_FAKE_MODE", "success")
        .env("CJ_FAKE_SELECTION", "1")
        .env("CJ_FAKE_ARGS", args)
        .env("CJ_FAKE_STDIN", stdin)
        .env("PATH", path_with_cj())
        .output()
        .expect("run PowerShell integration");
    assert_success(&output);
    assert_eq!(output.stdout, git.linked.as_os_str().as_encoded_bytes());
}

#[test]
fn powershell_tab_completion_selects_and_quotes_worktrees_without_changing_directory() {
    let git = GitFixture::new("windows-tab-completion-雪");
    let ordinary = git.main_nested.join("ordinary completion directory");
    fs::create_dir(&ordinary).expect("create ordinary completion directory");
    let tool = copy_tool(git.temp.path(), "fzf.exe");
    let config = git.temp.path().join("fzf completion config.toml");
    let source = git.temp.path().join("cj completion.ps1");
    let args = git.temp.path().join("fzf completion args");
    let stdin = git.temp.path().join("fzf completion stdin");
    write_config(&config, Path::new("missing-zoxide.exe"), &tool);
    let init = cj(&git.main_nested, git.temp.path())
        .arg("-C")
        .arg(&config)
        .args(["init", "powershell"])
        .output()
        .expect("generate PowerShell integration");
    assert_success(&init);
    fs::write(&source, init.stdout).expect("write PowerShell integration");

    let script = r#"$ErrorActionPreference = 'Stop'
. $env:CJ_SOURCE
$before = (Microsoft.PowerShell.Management\Get-Location).ProviderPath
$expected = $env:CJ_EXPECTED
foreach ($flag in @('-jw', '--jump-worktree')) {
    $matches = @(Complete-CjCdArgument $flag)
    if ($matches.Count -ne 1) { throw "expected one completion for $flag, got $($matches.Count)" }
    $selected = $matches[0].ListItemText
    if (-not [System.IO.Path]::GetFullPath($selected).Equals([System.IO.Path]::GetFullPath($expected), [System.StringComparison]::OrdinalIgnoreCase)) { throw "completion selected the wrong path: $selected" }
    $quoted = "'" + $selected.Replace("'", "''") + "'"
    if ($matches[0].CompletionText -cne $quoted) { throw "completion is not safely quoted: $($matches[0].CompletionText)" }
    $line = "cd $flag"
    $wired = @([System.Management.Automation.CommandCompletion]::CompleteInput($line, $line.Length, $null).CompletionMatches)
    if ($wired.Count -ne 1 -or -not [System.IO.Path]::GetFullPath($wired[0].ListItemText).Equals([System.IO.Path]::GetFullPath($expected), [System.StringComparison]::OrdinalIgnoreCase)) { throw "cd argument completer is not wired for $flag" }
    if ((Microsoft.PowerShell.Management\Get-Location).ProviderPath -cne $before) { throw 'completion changed directory' }
}
$env:CJ_FAKE_MODE = 'cancel'
$cancelled = @(Complete-CjCdArgument '-jw')
if ($cancelled.Count -ne 0) { throw 'cancellation replaced the command token' }
if ((Microsoft.PowerShell.Management\Get-Location).ProviderPath -cne $before) { throw 'cancellation changed directory' }
$directories = @(Complete-CjCdArgument 'ordinary')
if (-not ($directories | Where-Object { $_.ListItemText -like '*ordinary completion directory*' })) { throw 'normal directory completion was not preserved' }
[Console]::Out.Write('ok')
"#;
    let output = Command::new("pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .current_dir(&git.main_nested)
        .env("CJ_SOURCE", source)
        .env("CJ_EXPECTED", &git.linked)
        .env("CJ_FAKE_TOOL", "fzf")
        .env("CJ_FAKE_MODE", "success")
        .env("CJ_FAKE_SELECTION", "1")
        .env("CJ_FAKE_ARGS", args)
        .env("CJ_FAKE_STDIN", stdin)
        .env("PATH", path_with_cj())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .expect("run PowerShell completion");
    assert_success(&output);
    assert_eq!(output.stdout, b"ok");
    assert!(output.stderr.is_empty());
}

struct ToolFixture {
    temp: TempDir,
    cwd: PathBuf,
    destination: PathBuf,
    config: PathBuf,
    args: PathBuf,
}

impl ToolFixture {
    fn new(label: &str) -> Self {
        let temp = TempDir::new(label);
        let cwd = temp.path().join("current dir's path");
        let destination = temp.path().join("destination with a ' quote");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let zoxide = copy_tool(temp.path(), "zoxide.exe");
        let config = temp.path().join("config.toml");
        write_config(&config, &zoxide, Path::new("missing-fzf.exe"));
        let args = temp.path().join("tool args");
        Self {
            temp,
            cwd,
            destination,
            config,
            args,
        }
    }

    fn zoxide_command(&self) -> Command {
        let mut command = cj(&self.cwd, self.temp.path());
        command
            .arg("-C")
            .arg(&self.config)
            .env("CJ_FAKE_TOOL", "zoxide")
            .env("CJ_FAKE_MODE", "success")
            .env("CJ_FAKE_DEST", &self.destination)
            .env("CJ_FAKE_ARGS", &self.args);
        command
    }
}

fn copy_tool(root: &Path, name: &str) -> PathBuf {
    let destination = root.join("fake bin").join(name);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_cj-test-tool"), &destination).expect("copy fake tool");
    destination
}

fn path_with_cj() -> std::ffi::OsString {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_cj"));
    let mut paths = vec![binary.parent().unwrap().to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    env::join_paths(paths).expect("join PATH")
}

fn nul_strings(bytes: &[u8]) -> Vec<&str> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .map(|value| std::str::from_utf8(value).unwrap())
        .collect()
}

fn path_output(path: &Path) -> Vec<u8> {
    format!("{}\n", path.display()).into_bytes()
}
