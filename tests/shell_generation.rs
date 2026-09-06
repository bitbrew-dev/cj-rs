mod support;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use support::{TempDir, assert_success, cj};

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
            "repo",
            "home base",
            "code",
        ] {
            assert!(source.contains(expected), "{shell} omitted {expected:?}");
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
        generate_with_args(
            temp.path(),
            &config,
            ["init", "powershell", "--setup-key-binding"],
        ),
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
    }

    let completed = bash_complete(&completions, "COMP_WORDS=(cj init bash ''); COMP_CWORD=3");
    assert!(!completed.lines().any(|value| value == "bash"));
    assert!(
        completed
            .lines()
            .any(|value| value == "--setup-key-binding")
    );
    assert!(completed.lines().any(|value| value == "-o"));
    assert!(completed.lines().any(|value| value == "--output"));

    let completion_flags = bash_complete(
        &completions,
        "COMP_WORDS=(cj completions zsh ''); COMP_CWORD=3",
    );
    assert!(completion_flags.lines().any(|value| value == "-o"));
    assert!(completion_flags.lines().any(|value| value == "--output"));

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
