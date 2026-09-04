#![cfg(unix)]

mod support;

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use support::{GitFixture, TempDir, assert_success, cj, write_config, write_executable};

const ZOXIDE: &str = r#": > "$CJ_FAKE_ARGS"
for arg do
    printf '%s\000' "$arg" >> "$CJ_FAKE_ARGS"
done
case "$CJ_FAKE_MODE" in
    success) printf '%s\n' "$CJ_FAKE_DEST" ;;
    failure) printf 'fake zoxide failure\n' >&2; exit 9 ;;
esac"#;

const FZF: &str = r#": > "$CJ_FAKE_ARGS"
for arg do
    printf '%s\000' "$arg" >> "$CJ_FAKE_ARGS"
done
cat > "$CJ_FAKE_STDIN"
case "$CJ_FAKE_MODE" in
    success) printf '%s\tselected\000' "$CJ_FAKE_SELECTION" ;;
    cancel) exit 130 ;;
    failure) printf 'fake fzf failure\n' >&2; exit 7 ;;
esac"#;

#[test]
fn forced_zoxide_preserves_arguments_and_exact_destination() {
    let fixture = ToolFixture::new("zoxide-success");
    let output = fixture.zoxide_command("success").output().expect("run cj");

    assert_success(&output);
    assert_eq!(output.stdout, path_output(&fixture.destination));
    assert!(output.stderr.is_empty());
    assert_eq!(
        nul_strings(&fs::read(&fixture.args).expect("read zoxide arguments")),
        [
            "query",
            "--exclude",
            fixture.cwd.to_str().unwrap(),
            "--",
            "two words",
            "quo'te"
        ]
    );
}

#[test]
fn raw_and_no_zoxide_keep_distinct_resolver_semantics() {
    let fixture = GitFixture::new("resolver-modes");
    let zoxide = fixture.temp.path().join("fake bin/zoxide");
    let config = fixture.temp.path().join("resolver config.toml");
    let args = fixture.temp.path().join("zoxide args");
    let destination = fixture.temp.path().join("zoxide destination");
    fs::create_dir_all(&destination).expect("create zoxide destination");
    write_executable(&zoxide, ZOXIDE);
    write_config(&config, &zoxide, Path::new("/missing/fzf"));

    for target in ["top", "^^"] {
        let raw = cj(&fixture.main_nested, fixture.temp.path())
            .arg("-C")
            .arg(&config)
            .args(["-r", target])
            .output()
            .expect("run raw resolver");
        assert_success(&raw);
        assert_eq!(raw.stdout, format!("{target}\n").as_bytes());
    }

    let top = cj(&fixture.main_nested, fixture.temp.path())
        .arg("-C")
        .arg(&config)
        .args(["-Z", "top"])
        .output()
        .expect("run no-zoxide shortcut");
    assert_success(&top);
    assert_eq!(top.stdout, path_output(&fixture.main));

    let up = cj(&fixture.main_nested, fixture.temp.path())
        .arg("-C")
        .arg(&config)
        .args(["-Z", "^^"])
        .output()
        .expect("run no-zoxide ticker");
    assert_success(&up);
    assert_eq!(up.stdout, b"../..\n");
    assert!(!args.exists(), "raw and no-zoxide must not run zoxide");

    let forced = cj(&fixture.main_nested, fixture.temp.path())
        .arg("-C")
        .arg(&config)
        .args(["-z", "^^"])
        .env("CJ_FAKE_ARGS", &args)
        .env("CJ_FAKE_MODE", "success")
        .env("CJ_FAKE_DEST", &destination)
        .output()
        .expect("run forced zoxide");
    assert_success(&forced);
    assert_eq!(forced.stdout, path_output(&destination));
    assert_eq!(
        nul_strings(&fs::read(args).expect("read zoxide arguments")).last(),
        Some(&"^^")
    );
}

#[test]
fn forced_zoxide_failure_has_exact_process_contract() {
    let fixture = ToolFixture::new("zoxide-failure");
    let output = fixture.zoxide_command("failure").output().expect("run cj");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"cj: zoxide query failed: fake zoxide failure\n"
    );
}

#[test]
fn fzf_uses_nul_protocol_and_returns_selected_worktree() {
    let fixture = PickerFixture::new("fzf-success");
    let output = fixture.command("success").output().expect("run cj picker");

    assert_success(&output);
    assert_eq!(output.stdout, path_output(&fixture.git.linked));
    assert!(output.stderr.is_empty());
    assert_eq!(
        nul_strings(&fs::read(&fixture.args).expect("read fzf arguments")),
        [
            "--read0",
            "--print0",
            "--delimiter=\\t",
            "--with-nth=2..",
            "--nth=2..",
            "--height=40%",
            "--layout=reverse",
            "--border",
            "--prompt=cj worktree> "
        ]
    );
    let input = fs::read(&fixture.stdin).expect("read fzf input");
    let records = input
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert!(records[0].starts_with(b"0\t"));
    assert!(records[1].starts_with(b"1\t"));
    assert!(records.iter().any(|record| {
        record
            .windows(fixture.git.linked.as_os_str().as_bytes().len())
            .any(|window| window == fixture.git.linked.as_os_str().as_bytes())
    }));
}

#[test]
fn fzf_cancellation_and_failure_are_controlled() {
    let fixture = PickerFixture::new("fzf-errors");

    let cancelled = fixture.command("cancel").output().expect("cancel picker");
    assert_success(&cancelled);
    assert_eq!(cancelled.stdout, b"\n");
    assert!(cancelled.stderr.is_empty());

    let failed = fixture.command("failure").output().expect("fail picker");
    assert_eq!(failed.status.code(), Some(2));
    assert!(failed.stdout.is_empty());
    assert_eq!(
        failed.stderr,
        b"fake fzf failure\ncj: fzf exited with exit status: 7\n"
    );
}

#[test]
fn generated_wrappers_jump_and_propagate_failures() {
    let fixture = ToolFixture::new("shell-wrapper");
    let binary_dir = fixture.temp.path().join("binary dir");
    fs::create_dir_all(&binary_dir).expect("create binary directory");
    symlink(env!("CARGO_BIN_EXE_cj"), binary_dir.join("cj")).expect("link cj binary");
    let mut paths = vec![binary_dir];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(paths).expect("join PATH");
    let mut exercised = 0;

    for shell in ["bash", "zsh"] {
        let init = cj(&fixture.cwd, fixture.temp.path())
            .arg("-C")
            .arg(&fixture.config)
            .args(["init", shell])
            .output()
            .expect("render shell setup");
        assert_success(&init);

        let mut success = shell_command(
            shell,
            &init.stdout,
            "eval \"$1\"; cd -z \"$2\" \"$3\"; printf '%s\\n' \"$PWD\"",
        );
        success
            .current_dir(&fixture.cwd)
            .env("PATH", &path)
            .env("CJ_FAKE_ARGS", &fixture.args)
            .env("CJ_FAKE_MODE", "success")
            .env("CJ_FAKE_DEST", &fixture.destination);
        let success = match success.output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("cannot run {shell}: {error}"),
        };
        exercised += 1;
        assert_success(&success);
        assert_eq!(success.stdout, path_output(&fixture.destination));
        assert!(success.stderr.is_empty());

        let failed = shell_command(shell, &init.stdout, "eval \"$1\"; cd -z \"$2\" \"$3\"")
            .current_dir(&fixture.cwd)
            .env("PATH", &path)
            .env("CJ_FAKE_ARGS", &fixture.args)
            .env("CJ_FAKE_MODE", "failure")
            .env("CJ_FAKE_DEST", &fixture.destination)
            .output()
            .expect("run available shell");
        assert_eq!(failed.status.code(), Some(2));
        assert!(failed.stdout.is_empty());
        assert_eq!(
            failed.stderr,
            b"cj: zoxide query failed: fake zoxide failure\n"
        );
    }
    assert!(exercised > 0, "neither bash nor zsh is available");
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
        let zoxide = temp.path().join("fake bin's dir/zoxide");
        let config = temp.path().join("config dir/it's config.toml");
        let args = temp.path().join("zoxide args");
        fs::create_dir_all(&cwd).expect("create current directory");
        fs::create_dir_all(&destination).expect("create destination");
        write_executable(&zoxide, ZOXIDE);
        write_config(&config, &zoxide, Path::new("/missing/fzf"));
        Self {
            temp,
            cwd,
            destination,
            config,
            args,
        }
    }

    fn zoxide_command(&self, mode: &str) -> Command {
        let mut command = cj(&self.cwd, self.temp.path());
        command
            .arg("-C")
            .arg(&self.config)
            .args(["-z", "two words", "quo'te"])
            .env("CJ_FAKE_ARGS", &self.args)
            .env("CJ_FAKE_MODE", mode)
            .env("CJ_FAKE_DEST", &self.destination);
        command
    }
}

struct PickerFixture {
    git: GitFixture,
    config: PathBuf,
    args: PathBuf,
    stdin: PathBuf,
}

impl PickerFixture {
    fn new(label: &str) -> Self {
        let git = GitFixture::new(label);
        let fzf = git.temp.path().join("fake bin's dir/fzf");
        let config = git.temp.path().join("config dir/it's config.toml");
        let args = git.temp.path().join("fzf args");
        let stdin = git.temp.path().join("fzf stdin");
        write_executable(&fzf, FZF);
        write_config(&config, Path::new("/missing/zoxide"), &fzf);
        Self {
            git,
            config,
            args,
            stdin,
        }
    }

    fn command(&self, mode: &str) -> Command {
        let mut command = cj(&self.git.main_nested, self.git.temp.path());
        command
            .arg("-C")
            .arg(&self.config)
            .arg("--pick-worktree")
            .env("CJ_FAKE_ARGS", &self.args)
            .env("CJ_FAKE_STDIN", &self.stdin)
            .env("CJ_FAKE_MODE", mode)
            .env("CJ_FAKE_SELECTION", "1");
        command
    }
}

fn shell_command(shell: &str, setup: &[u8], script: &str) -> Command {
    let mut command = Command::new(shell);
    if shell == "bash" {
        command.args(["--noprofile", "--norc"]);
    } else {
        command.arg("-f");
    }
    command
        .args(["-c", script, "_"])
        .arg(OsStr::from_bytes(setup))
        .args(["two words", "quo'te"]);
    command
}

fn path_output(path: &Path) -> Vec<u8> {
    format!("{}\n", path.display()).into_bytes()
}

fn nul_strings(bytes: &[u8]) -> Vec<&str> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| std::str::from_utf8(field).expect("fake arguments are UTF-8"))
        .collect()
}
