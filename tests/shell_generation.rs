mod support;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use support::{GitFixture, TempDir, assert_success, cj};

#[test]
fn generated_sources_parse_in_available_shells() {
    let temp = TempDir::new("generated-shell-source");
    let config = temp.path().join("it's config.toml");
    fs::write(
        &config,
        format!(
            "[aliases]\n\"work space\" = \"{}\"\n\"it's;$()\" = \"{}\"\n",
            toml_string(temp.path()),
            toml_string(temp.path())
        ),
    )
    .expect("write completion config");

    for shell in ["bash", "zsh", "nu", "powershell"] {
        for command in ["init", "completions"] {
            let source = generate(temp.path(), &config, command, shell);
            parse_source(temp.path(), command, shell, &source);
        }
    }
}

#[test]
fn generation_is_deterministic_and_keeps_source_on_stdout() {
    let temp = TempDir::new("deterministic-shell-source");
    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        format!(
            "[keywords]\ntop = [\"repo\", \"repo\"]\nmain-worktree = [\"home base\"]\n\n[aliases]\ncode = \"{}\"\n",
            toml_string(temp.path())
        ),
    )
    .expect("write completion config");

    for shell in ["bash", "zsh", "nu", "powershell"] {
        let first = cj(temp.path(), temp.path())
            .args(["-C", config.to_str().unwrap(), "completions", shell])
            .output()
            .expect("generate completions");
        let second = cj(temp.path(), temp.path())
            .args(["-C", config.to_str().unwrap(), "completions", shell])
            .output()
            .expect("regenerate completions");
        assert_success(&first);
        assert_success(&second);
        assert!(first.stderr.is_empty());
        assert_eq!(first.stdout, second.stdout);

        let source = String::from_utf8(first.stdout).expect("generated source is UTF-8");
        for expected in [
            "completions",
            "config",
            "init",
            "mounts",
            "table",
            "json",
            "bash",
            "zsh",
            "nu",
            "powershell",
            "--setup-key-binding",
            "--no-setup-key-binding",
            "repo",
            "home base",
            "code",
        ] {
            assert!(source.contains(expected), "{shell} omitted {expected:?}");
        }
    }
}

#[test]
fn init_installs_binding_by_default_and_opt_out_keeps_shell_integration() {
    let temp = TempDir::new("default-init-binding");
    let config = temp.path().join("config.toml");
    fs::write(&config, "").expect("write default config");

    for (shell, binding, wrapper, completion) in [
        (
            "bash",
            "_cj_key_widget() {",
            "function cd()",
            "complete -F _cj_complete_cd cd",
        ),
        (
            "zsh",
            "_cj_key_widget() {",
            "function cd()",
            "compdef _cj_complete_cd cd",
        ),
        (
            "powershell",
            "Set-PSReadLineKeyHandler -Chord",
            "function global:cd",
            "function global:TabExpansion2",
        ),
        (
            "nu",
            "name: cj-directory",
            "export def --env --wrapped __cj_cd",
            "_cj-complete-cd",
        ),
    ] {
        let default = generate(temp.path(), &config, "init", shell);
        let legacy =
            generate_with_args(temp.path(), &config, ["init", shell, "--setup-key-binding"]);
        let disabled = generate_with_args(
            temp.path(),
            &config,
            ["init", shell, "--no-setup-key-binding"],
        );
        assert_eq!(
            default, legacy,
            "{shell}: compatibility flag changed output"
        );
        assert!(
            disabled.contains(wrapper),
            "{shell}: opt-out removed cd wrapper"
        );
        assert!(
            disabled.contains(completion),
            "{shell}: opt-out removed cd completion"
        );
        assert!(
            !disabled.contains(binding),
            "{shell}: opt-out installed binding"
        );
        assert!(
            default.contains(binding),
            "{shell}: default omitted binding"
        );
        if shell == "nu" {
            assert!(default.contains("_cj-key-widget"));
        }
        parse_source(temp.path(), "init-no-binding", shell, &disabled);
    }
}

#[test]
fn init_respects_configured_binding_disablement() {
    let temp = TempDir::new("disabled-init-binding");
    let config = temp.path().join("config.toml");
    for settings in [
        "[key-bindings]\nmacos = 'none'\nlinux = 'none'\nwindows = 'none'\n",
        "[key-bindings]\nmacos = { behaviors = [] }\nlinux = { behaviors = [] }\nwindows = { behaviors = [] }\n",
    ] {
        fs::write(&config, settings).expect("write disabled binding config");
        for shell in ["bash", "zsh", "nu", "powershell"] {
            let default = generate(temp.path(), &config, "init", shell);
            let disabled = generate_with_args(
                temp.path(),
                &config,
                ["init", shell, "--no-setup-key-binding"],
            );
            assert_eq!(
                default, disabled,
                "{shell}: disabled config installed binding"
            );
        }
    }
}

#[test]
fn shell_output_creates_parents_and_replaces_the_file() {
    let temp = TempDir::new("init-output");
    for command in ["init", "completions"] {
        let path = temp.path().join(format!("missing/{command}/source.zsh"));
        let expected = cj(temp.path(), temp.path())
            .args([command, "zsh"])
            .output()
            .expect("generate shell source on stdout");
        assert_success(&expected);

        for flag in ["-o", "--output"] {
            if path.exists() {
                fs::write(&path, "stale source").expect("write stale shell source");
            }
            let output = cj(temp.path(), temp.path())
                .args([command, "zsh", flag])
                .arg(&path)
                .output()
                .expect("write shell source to file");
            assert_success(&output);
            assert!(output.stdout.is_empty());
            assert!(output.stderr.is_empty());
            assert_eq!(fs::read(&path).expect("read shell source"), expected.stdout);
        }
    }
}

#[test]
fn nushell_integration_changes_the_calling_shell_directory() {
    if !available("nu") {
        eprintln!("skipping Nushell behavior test: nu is unavailable");
        return;
    }

    let temp = TempDir::new("nu-parent-shell");
    let root = temp.path().join("target directory's child");
    let destination = root.join("one/two");
    let config = temp.path().join("it's config.toml");
    fs::create_dir_all(&destination).expect("create destination");
    fs::write(
        &config,
        format!(
            "[aliases]\n\"work space\" = \"{}\"\n",
            toml_string(&destination)
        ),
    )
    .expect("write config");
    let integration = generate(temp.path(), &config, "init", "nu");
    let source = temp.path().join("cj.nu");
    fs::write(&source, integration).expect("write Nushell integration");

    let output = Command::new("nu")
        .args([
            "--no-config-file",
            "-c",
            &format!(
                "use {} *; let names = ('cd work' | commandline complete --detailed | get value); if 'work space' not-in $names {{ error make {{ msg: $'missing configured cd completion: ($names | to nuon)' }} }}; let paths = ('cd target' | commandline complete --detailed | get value); if not ($paths | any {{ |path| $path | str contains 'target directory' }}) {{ error make {{ msg: $'missing directory completion: ($paths | to nuon)' }} }}; cd -P $env.CJ_TEST_ROOT; cd $env.CJ_TEST_ALIAS; cd ^^; cd v; print $env.PWD",
                quote_nu(source.to_str().unwrap())
            ),
        ])
        .current_dir(temp.path())
        .env("CJ_TEST_ALIAS", "work space")
        .env("CJ_TEST_ROOT", &root)
        .env("PATH", path_with_cj())
        .env("HOME", temp.path())
        .output()
        .expect("run Nushell integration");
    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim_end(),
        root.join("one").to_string_lossy()
    );
}

#[test]
fn powershell_integration_and_completion_work_when_available() {
    if !available("pwsh") {
        eprintln!("skipping PowerShell behavior test: pwsh is unavailable");
        return;
    }

    let temp = TempDir::new("powershell-parent-shell");
    let root = temp.path().join("target [directory] with space");
    let destination = root.join("one/two");
    let config = temp.path().join("it's config.toml");
    fs::create_dir_all(&destination).expect("create destination");
    fs::write(
        &config,
        format!(
            "[aliases]\n\"work space\" = \"{}\"\n",
            toml_string(&destination)
        ),
    )
    .expect("write config");

    let integration_path = temp.path().join("cj.ps1");
    let completions_path = temp.path().join("cj-completions.ps1");
    fs::write(
        &integration_path,
        generate(temp.path(), &config, "init", "powershell"),
    )
    .expect("write PowerShell integration");
    fs::write(
        &completions_path,
        generate(temp.path(), &config, "completions", "powershell"),
    )
    .expect("write PowerShell completions");

    let script = r#"$ErrorActionPreference = 'Stop'
. $env:CJ_INIT_SOURCE
. $env:CJ_COMPLETION_SOURCE
cd $env:CJ_TEST_ALIAS
cd '^^'
cd 'v'
$text = 'cj config init --p'
$matches = [System.Management.Automation.CommandCompletion]::CompleteInput($text, $text.Length, $null).CompletionMatches.CompletionText
if ('--preamp' -notin $matches) { throw 'missing --preamp completion' }
if (Get-Command Get-PSReadLineKeyHandler -ErrorAction SilentlyContinue) {
    if ($null -eq (Get-PSReadLineKeyHandler -Chord 'Ctrl+o')) { throw 'missing cj PSReadLine binding' }
}
[Console]::Out.Write((Microsoft.PowerShell.Management\Get-Location).ProviderPath)
"#;
    let output = Command::new("pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .current_dir(temp.path())
        .env("CJ_INIT_SOURCE", &integration_path)
        .env("CJ_COMPLETION_SOURCE", &completions_path)
        .env("CJ_TEST_ALIAS", "work space")
        .env("PATH", path_with_cj())
        .env("HOME", temp.path())
        .output()
        .expect("run PowerShell integration");
    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        root.join("one").to_string_lossy()
    );
}

#[test]
fn powershell_smart_quotes_round_trip_in_source_and_completions() {
    if !available("pwsh") {
        eprintln!("skipping PowerShell smart quote test: pwsh is unavailable");
        return;
    }
    let fixture = GitFixture::new("powershell-smart-quotes");
    let temp = &fixture.temp;
    let worktree = temp.path().join("worktree's ‘left’ ‚low‛");
    assert_success(
        &Command::new("git")
            .current_dir(&fixture.main)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            .args(["worktree", "move"])
            .arg(&fixture.linked)
            .arg(&worktree)
            .output()
            .unwrap(),
    );
    let destination = temp.path().join("target's ‘left’ ‚low‛ 雪");
    let config = temp.path().join("config's ‘left’ ‚low‛.toml");
    fs::create_dir_all(&destination).unwrap();
    let mut aliases = vec!["it’s-here".to_string()];
    for (index, quote) in ['\'', '‘', '’', '‚', '‛'].into_iter().enumerate() {
        aliases.push(format!(
            "quote{index}{quote}; $global:CjInjected=1; #{quote}"
        ));
    }
    let mut contents = String::from("[aliases]\n");
    for name in &aliases {
        contents.push_str(&format!(
            "{} = \"{}\"\n",
            serde_json::to_string(name).unwrap(),
            toml_string(&destination)
        ));
    }
    fs::write(&config, contents).unwrap();
    let integration = temp.path().join("init.ps1");
    let completions = temp.path().join("completions.ps1");
    fs::write(
        &integration,
        generate(temp.path(), &config, "init", "powershell"),
    )
    .unwrap();
    fs::write(
        &completions,
        generate(temp.path(), &config, "completions", "powershell"),
    )
    .unwrap();

    let script = r#"$ErrorActionPreference = 'Stop'
$global:CjInjected = 0
function Test-SameDirectory([string]$Actual, [string]$Expected) {
    # Git uses '/', while Windows PathBuf and Get-Location use '\'.
    $comparison = if ($IsWindows) { [System.StringComparison]::OrdinalIgnoreCase } else { [System.StringComparison]::Ordinal }
    return [System.IO.Path]::GetFullPath($Actual).Equals([System.IO.Path]::GetFullPath($Expected), $comparison)
}
. $env:CJ_INIT_SOURCE
. $env:CJ_COMPLETION_SOURCE
if ($script:__cj_config[1] -cne $env:CJ_TEST_CONFIG) { throw 'config path changed' }
$line = 'cd -jw'
$before = (Get-Location).ProviderPath
$result = TabExpansion2 $line $line.Length
$match = @($result.CompletionMatches | Where-Object { Test-SameDirectory $_.ListItemText $env:CJ_TEST_WORKTREE })
if ($match.Count -ne 1) { throw "missing smart-quote worktree completion: expected $env:CJ_TEST_WORKTREE; got $($result.CompletionMatches.ListItemText -join ', ')" }
if ((Get-Location).ProviderPath -cne $before) { throw 'Tab changed directory' }
$literalPath = & ([scriptblock]::Create($match[0].CompletionText))
if ($literalPath -isnot [string] -or $literalPath -cne $match[0].ListItemText) { throw 'completion changed literal worktree path' }
$completed = $line.Remove($result.ReplacementIndex, $result.ReplacementLength).Insert($result.ReplacementIndex, $match[0].CompletionText)
& ([scriptblock]::Create($completed))
if (-not (Test-SameDirectory (Get-Location).ProviderPath $env:CJ_TEST_WORKTREE)) { throw 'completed worktree path changed' }
$aliases = @($env:CJ_TEST_ALIASES | ConvertFrom-Json)
foreach ($command in @('cj', 'cd')) {
    foreach ($expected in $aliases) {
        $line = $command + ' ' + $expected.Substring(0, 2)
        $candidates = if ($command -ceq 'cd') {
            Complete-CjCdArgument $expected.Substring(0, 2)
        } else {
            [System.Management.Automation.CommandCompletion]::CompleteInput($line, $line.Length, $null).CompletionMatches
        }
        $matches = @($candidates | Where-Object ListItemText -CEQ $expected)
        if ($matches.Count -ne 1) { throw "missing $command completion: $expected" }
        $literal = $matches[0].CompletionText
        $value = & ([scriptblock]::Create($literal))
        if ($value -isnot [string] -or $value -cne $expected) { throw "completion changed: $expected" }
        if ($command -ceq 'cd') {
            & ([scriptblock]::Create('cd ' + $literal))
            if (-not (Test-SameDirectory (Get-Location).ProviderPath $env:CJ_TEST_DESTINATION)) { throw 'completed alias failed to resolve' }
        }
        if ($global:CjInjected -ne 0) { throw 'completion executed embedded code' }
    }
}
if ($global:CjInjected -ne 0) { throw 'generated source executed embedded code' }
"#;
    let output = Command::new("pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .current_dir(&fixture.main)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env("PATH", path_with_cj())
        .env("HOME", temp.path())
        .env("CJ_INIT_SOURCE", &integration)
        .env("CJ_COMPLETION_SOURCE", &completions)
        .env("CJ_TEST_CONFIG", &config)
        .env("CJ_TEST_WORKTREE", &worktree)
        .env("CJ_TEST_ALIASES", serde_json::to_string(&aliases).unwrap())
        .env("CJ_TEST_DESTINATION", &destination)
        .output()
        .unwrap();
    assert_success(&output);
}

#[cfg(unix)]
#[test]
fn bash_completion_handles_nested_commands_and_spaced_names() {
    if !available("bash") {
        eprintln!("skipping Bash completion test: bash is unavailable");
        return;
    }

    let temp = TempDir::new("bash-completion");
    let config = temp.path().join("config.toml");
    fs::write(&config, "[aliases]\n\"work space\" = \"/tmp\"\n").expect("write config");
    let completions = generate(temp.path(), &config, "completions", "bash");

    let nested = bash_complete(&completions, "COMP_WORDS=(cj config init ''); COMP_CWORD=3");
    assert!(nested.lines().any(|value| value == "--preamp"));
    assert!(!nested.lines().any(|value| value == "init"));

    let destinations = bash_complete(&completions, "COMP_WORDS=(cj 'work'); COMP_CWORD=1");
    assert!(destinations.lines().any(|value| value == "work space"));

    for setup in [
        "COMP_WORDS=(cj config -- ''); COMP_CWORD=3",
        "COMP_WORDS=(cj init -z ''); COMP_CWORD=3",
    ] {
        let forced = bash_complete(&completions, setup);
        assert!(!forced.lines().any(|value| value == "bash"));
        assert!(!forced.lines().any(|value| value == "--setup-key-binding"));
        assert!(
            !forced
                .lines()
                .any(|value| value == "--no-setup-key-binding")
        );
    }

    let completed = bash_complete(&completions, "COMP_WORDS=(cj init bash ''); COMP_CWORD=3");
    assert!(!completed.lines().any(|value| value == "bash"));
    assert!(
        completed
            .lines()
            .any(|value| value == "--setup-key-binding")
    );
    assert!(
        completed
            .lines()
            .any(|value| value == "--no-setup-key-binding")
    );
    assert!(completed.lines().any(|value| value == "-o"));
    assert!(completed.lines().any(|value| value == "--output"));

    let completion_flags = bash_complete(
        &completions,
        "COMP_WORDS=(cj completions zsh ''); COMP_CWORD=3",
    );
    assert!(completion_flags.lines().any(|value| value == "-o"));
    assert!(completion_flags.lines().any(|value| value == "--output"));
    assert!(
        !completion_flags
            .lines()
            .any(|value| value == "--setup-key-binding")
    );
    assert!(
        !completion_flags
            .lines()
            .any(|value| value == "--no-setup-key-binding")
    );

    let worktree = bash_complete(&completions, "COMP_WORDS=(cj -w ''); COMP_CWORD=2");
    assert!(worktree.lines().any(|value| value == "--relative"));
    assert!(!worktree.lines().any(|value| value == "init"));

    let literal = bash_complete(&completions, "COMP_WORDS=(cj -- -f ''); COMP_CWORD=3");
    assert!(!literal.lines().any(|value| value == "table"));
    assert!(!literal.lines().any(|value| value == "--config"));
}

fn generate(root: &Path, config: &Path, command: &str, shell: &str) -> String {
    generate_with_args(root, config, [command, shell])
}

fn generate_with_args<const N: usize>(root: &Path, config: &Path, args: [&str; N]) -> String {
    let output = cj(root, root)
        .arg("-C")
        .arg(config)
        .args(args)
        .output()
        .expect("generate shell source");
    assert_success(&output);
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).expect("generated source is UTF-8")
}

fn parse_source(root: &Path, command: &str, shell: &str, source: &str) {
    let extension = match shell {
        "powershell" => "ps1",
        other => other,
    };
    let path = root.join(format!("{command}.{extension}"));
    fs::write(&path, source).expect("write generated source");

    let output = match shell {
        "bash" if available("bash") => Command::new("bash").arg("-n").arg(&path).output(),
        "zsh" if available("zsh") => Command::new("zsh").arg("-n").arg(&path).output(),
        "nu" if available("nu") => Command::new("nu")
            .args([
                "--no-config-file",
                "-c",
                &format!("source {}", quote_nu(path.to_str().unwrap())),
            ])
            .output(),
        "powershell" if available("pwsh") => Command::new("pwsh")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$source = Get-Content -Raw -LiteralPath $env:CJ_SOURCE; [scriptblock]::Create($source) | Out-Null",
            ])
            .env("CJ_SOURCE", &path)
            .output(),
        _ => {
            eprintln!("skipping {shell} parse test: interpreter is unavailable");
            return;
        }
    }
    .expect("run shell parser");
    assert_success(&output);
}

#[cfg(unix)]
fn bash_complete(source: &str, setup: &str) -> String {
    let output = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-c",
            &format!("eval \"$1\"; {setup}; _cj_complete; printf '%s\\n' \"${{COMPREPLY[@]}}\""),
            "_",
            source,
        ])
        .output()
        .expect("run Bash completion");
    assert_success(&output);
    String::from_utf8(output.stdout).unwrap()
}

fn available(program: &str) -> bool {
    let available = Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    assert!(
        available
            || (env::var_os("CJ_REQUIRE_SHELLS").is_none()
                && !(program == "pwsh" && env::var_os("CJ_REQUIRE_POWERSHELL").is_some())),
        "required shell interpreter is unavailable: {program}"
    );
    available
}

fn path_with_cj() -> std::ffi::OsString {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_cj"));
    let mut paths = vec![binary.parent().unwrap().to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    env::join_paths(paths).expect("join PATH")
}

fn quote_nu(value: &str) -> String {
    format!("r#'{value}'#")
}

fn toml_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}
