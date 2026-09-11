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
fn powershell_wrapper_enters_main_worktree_without_the_picker() {
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

    for flag in ["-jw", "--jump-worktree"] {
        let output = Command::new("pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            ". $env:CJ_SOURCE; $result = 0; try { cd $env:CJ_JUMP_FLAG } catch { [Console]::Error.Write($_.Exception.Message); $result = 2 }; [Console]::Out.Write((Get-Location).ProviderPath); exit $result",
        ])
        .current_dir(&git.main_nested)
        .env("CJ_SOURCE", &source)
        .env("CJ_JUMP_FLAG", flag)
        .env("CJ_FAKE_TOOL", "fzf")
        .env("CJ_FAKE_MODE", "success")
        .env("CJ_FAKE_SELECTION", "1")
        .env("CJ_FAKE_ARGS", &args)
        .env("CJ_FAKE_STDIN", &stdin)
        .env("PATH", path_with_cj())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("run PowerShell integration");
        assert_success(&output);
        let actual = dunce::canonicalize(String::from_utf8(output.stdout).unwrap()).unwrap();
        assert_eq!(actual, dunce::canonicalize(&git.main).unwrap());
        assert!(output.stderr.is_empty());
    }
    assert!(
        !args.exists() && !stdin.exists(),
        "bare worktree Enter must not invoke fzf"
    );
}

#[test]
fn powershell_tab_completion_offers_native_worktree_candidates() {
    let git = GitFixture::new("windows-tab-completion-雪");
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
function global:TabExpansion2 {
    param([string]$inputScript, [int]$cursorColumn = $inputScript.Length, [hashtable]$options = $null); $global:CJ_DELEGATED += 1
    $items = [System.Collections.ObjectModel.Collection[System.Management.Automation.CompletionResult]]::new(); [void]$items.Add([System.Management.Automation.CompletionResult]::new('native-fallback', 'native-fallback', 'ProviderContainer', 'native-fallback')); [System.Management.Automation.CommandCompletion]::new($items, -1, 0, 0)
}
$global:CJ_DELEGATED = 0; . $env:CJ_SOURCE
$before = (Microsoft.PowerShell.Management\Get-Location).ProviderPath; $expected = @($env:CJ_MAIN, $env:CJ_LINKED)
foreach ($flag in @('-jw', '--jump-worktree')) {
    $matches = @(Complete-CjCdArgument $flag)
    if ($matches.Count -ne 2) { throw "expected two completions for $flag, got $($matches.Count)" }
    foreach ($match in $matches) {
        $selected = $match.ListItemText; if (-not ($expected | Where-Object { [System.IO.Path]::GetFullPath($_).Equals([System.IO.Path]::GetFullPath($selected), [System.StringComparison]::OrdinalIgnoreCase) })) { throw "unexpected worktree: $selected" }
        $quoted = "'" + $selected.Replace("'", "''") + "'"; if ($match.CompletionText -cne $quoted) { throw "completion is not safely quoted: $($match.CompletionText)" }
    }
    $line = "cd $flag"; $completion = TabExpansion2 -inputScript $line -cursorColumn $line.Length
    if ($completion.CompletionMatches.Count -ne 2) { throw "cd completion is not wired for $flag" }
    if (($completion.ReplacementIndex -ne 3) -or ($completion.ReplacementLength -ne $flag.Length)) { throw "wrong replacement span for $flag" }
    if ((Microsoft.PowerShell.Management\Get-Location).ProviderPath -cne $before) { throw 'completion changed directory' }
}
foreach ($line in @('cd ordinary', 'cd -j')) {
    $delegated = TabExpansion2 -inputScript $line -cursorColumn $line.Length; if ($delegated.CompletionMatches[0].ListItemText -cne 'native-fallback') { throw "ordinary completion was not delegated: $line" }
}
if ($global:CJ_DELEGATED -ne 2) { throw 'saved TabExpansion2 was not called twice' }
if ((Microsoft.PowerShell.Management\Get-Location).ProviderPath -cne $before) { throw 'completion changed directory' }
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
        .env("CJ_MAIN", &git.main)
        .env("CJ_LINKED", &git.linked)
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
    assert!(
        !git.temp.path().join("fzf completion args").exists()
            && !git.temp.path().join("fzf completion stdin").exists()
    );
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
