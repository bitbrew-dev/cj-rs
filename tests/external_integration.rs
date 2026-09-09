#![cfg(unix)]

mod support;

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
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
fn raw_resolver_preserves_non_utf8_path_bytes() {
    let temp = TempDir::new("non-utf8-resolver");
    let target = temp
        .path()
        .join(OsString::from_vec(b"invalid-utf8-\xff".to_vec()));
    #[cfg(not(target_os = "macos"))]
    fs::create_dir(&target).expect("create non-UTF-8 directory");

    let output = cj(temp.path(), temp.path())
        .arg("-r")
        .arg(&target)
        .output()
        .expect("resolve non-UTF-8 path");
    assert_success(&output);
    assert_eq!(
        output.stdout,
        [target.as_os_str().as_bytes(), b"\n"].concat()
    );
    assert!(output.stderr.is_empty());
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
fn unsafe_ticker_config_is_rejected_before_shell_generation() {
    let temp = TempDir::new("unsafe-ticker");
    let config = temp.path().join("unsafe ticker.toml");
    fs::write(
        &config,
        "[tickers]\nnavigate_up = \"^\"\nnavigate_down = \">\"\n",
    )
    .expect("write unsafe config");

    let output = cj(temp.path(), temp.path())
        .arg("-C")
        .arg(&config)
        .args(["init", "bash"])
        .output()
        .expect("run cj init");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("error is UTF-8");
    assert!(stderr.contains("tickers.navigate_down \">\" is not allowed"));
    assert!(stderr.contains("allowed values: ^, v, u, d, j, k"));
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
fn generated_wrappers_require_tab_for_worktree_selection() {
    let fixture = PickerFixture::new("worktree-tab-only");
    let binary_dir = fixture.git.temp.path().join("binary dir");
    fs::create_dir_all(&binary_dir).expect("create binary directory");
    symlink(env!("CARGO_BIN_EXE_cj"), binary_dir.join("cj")).expect("link cj binary");
    let mut paths = vec![binary_dir];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(paths).expect("join PATH");
    let mut exercised = 0;

    for shell in ["bash", "zsh"] {
        let init = cj(&fixture.git.main_nested, fixture.git.temp.path())
            .arg("-C")
            .arg(&fixture.config)
            .args(["init", shell])
            .output()
            .expect("render shell setup");
        assert_success(&init);

        for flag in ["-jw", "--jump-worktree"] {
            let mut command = shell_with_setup(
                shell,
                &init.stdout,
                "eval \"$1\"; cd \"$2\"; result=$?; printf '%s\\n' \"$PWD\"; exit \"$result\"",
            );
            command
                .arg(flag)
                .current_dir(&fixture.git.main_nested)
                .env("PATH", &path)
                .env("CJ_FAKE_ARGS", &fixture.args)
                .env("CJ_FAKE_STDIN", &fixture.stdin)
                .env("CJ_FAKE_MODE", "success")
                .env("CJ_FAKE_SELECTION", "1");
            let output = match command.output() {
                Ok(output) => output,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("cannot run {shell}: {error}"),
            };
            exercised += 1;
            assert!(
                !output.status.success(),
                "{shell} accepted {flag} without Tab"
            );
            assert_eq!(output.stdout, path_output(&fixture.git.main_nested));
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("type cd -jw and press Tab to select a worktree")
            );
        }
    }
    assert!(exercised > 0, "neither bash nor zsh is available");
    assert!(
        !fixture.args.exists() && !fixture.stdin.exists(),
        "executing the Tab token must not invoke fzf"
    );
}

#[test]
fn generated_posix_tab_completion_offers_native_worktree_candidates() {
    let fixture = PickerFixture::new("native-tab-completion-雪");
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_cj"));
    let mut paths = vec![binary.parent().unwrap().to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(paths).expect("join PATH");
    let mut exercised = 0;

    for shell in ["bash", "zsh"] {
        let init = cj(&fixture.git.main_nested, fixture.git.temp.path())
            .arg("-C")
            .arg(&fixture.config)
            .args(["init", shell])
            .output()
            .expect("render shell setup");
        assert_success(&init);

        for (flag, fzf) in [
            ("-jw", fixture.git.temp.path().join("fake bin's dir/fzf")),
            (
                "--jump-worktree",
                fixture.git.temp.path().join("missing-fzf"),
            ),
        ] {
            write_config(&fixture.config, Path::new("/missing/zoxide"), &fzf);
            let script = if shell == "bash" {
                concat!(
                    "eval \"$1\"; COMP_WORDS=(cd \"$2\"); COMP_CWORD=1; ",
                    "_cj_complete_cd; printf '%s\\000' \"${COMPREPLY[@]}\" ",
                    "\"${COMP_WORDS[COMP_CWORD]}\" \"$PWD\""
                )
            } else {
                concat!(
                    "eval \"$1\"; typeset -A compstate; typeset -ga words; words=(cd \"$2\"); integer CURRENT=2; ",
                    "compadd() { local emit=; for value in \"$@\"; do ",
                    "if [[ -n \"$emit\" ]]; then printf '%s\\000' \"$value\"; ",
                    "elif [[ \"$value\" == -- ]]; then emit=1; fi; done; }; ",
                    "_cj_complete_cd; printf '%s\\000' \"${words[CURRENT]}\" \"$PWD\""
                )
            };
            let mut command = shell_with_setup(shell, &init.stdout, script);
            command
                .arg(flag)
                .current_dir(&fixture.git.main_nested)
                .env("PATH", &path);
            let output = match command.output() {
                Ok(output) => output,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("cannot run {shell}: {error}"),
            };
            exercised += 1;
            assert_success(&output);
            let fields = nul_strings(&output.stdout);
            assert_eq!(fields.len(), 4);
            assert!(fields[..2].contains(&fixture.git.main.to_str().unwrap()));
            assert!(fields[..2].contains(&fixture.git.linked.to_str().unwrap()));
            assert_eq!(fields[2], flag);
            assert_eq!(fields[3], fixture.git.main_nested.to_str().unwrap());
        }

        let script = if shell == "bash" {
            concat!(
                "eval \"$1\"; _cd() { COMPREPLY=(native-fallback); }; ",
                "for value in ordinary -j; do COMP_WORDS=(cd \"$value\"); COMP_CWORD=1; ",
                "COMPREPLY=(); _cj_complete_cd; printf '%s\\000' \"${COMPREPLY[@]}\"; done; ",
                "printf '%s\\000' \"$PWD\""
            )
        } else {
            concat!(
                "eval \"$1\"; _cd() { printf 'native-fallback\\000'; }; ",
                "compadd() { :; }; for value in ordinary -j; do ",
                "typeset -ga words; words=(cd \"$value\"); integer CURRENT=2; ",
                "_cj_complete_cd; done; printf '%s\\000' \"$PWD\""
            )
        };
        let mut fallback = shell_with_setup(shell, &init.stdout, script);
        fallback
            .current_dir(&fixture.git.main_nested)
            .env("PATH", &path);
        let fallback = fallback.output().expect("run native cd completion");
        assert_success(&fallback);
        assert_eq!(
            nul_strings(&fallback.stdout),
            [
                "native-fallback",
                "native-fallback",
                fixture.git.main_nested.to_str().unwrap()
            ]
        );
    }
    assert!(exercised > 0, "neither bash nor zsh is available");
    assert!(
        !fixture.args.exists() && !fixture.stdin.exists(),
        "completion invoked fzf"
    );
}

#[test]
fn generated_zsh_completion_replaces_jump_tokens_in_real_zle() {
    if Command::new("zsh").arg("--version").output().is_err() {
        return;
    }
    let fixture = PickerFixture::new("zle-worktree-completion-雪");
    let init = cj(&fixture.git.main_nested, fixture.git.temp.path())
        .arg("-C")
        .arg(&fixture.config)
        .args(["init", "zsh"])
        .output()
        .expect("render zsh setup");
    assert_success(&init);
    let script = fixture.git.temp.path().join("completion.zsh");
    let results = fixture.git.temp.path().join("completion-results");
    let mut setup = init.stdout;
    setup.extend_from_slice(
        br#"
autoload -Uz compinit
compinit -i -D
_test_complete() {
    _cj_complete_cd
    printf '%s\000' "$compstate[nmatches]" >> "$CJ_TEST_RESULTS"
}
zle -C _test_complete_widget complete-word _test_complete
zle-line-init() {
    local flag
    local -a parsed
    for flag in -jw --jump-worktree -jw; do
        BUFFER="cd $flag"
        CURSOR=$#BUFFER
        # Also exercise a cursor inside the token, where SUFFIX is nonempty.
        [[ -e "$CJ_TEST_RESULTS" && "$flag" == -jw ]] && (( CURSOR-- ))
        zle _test_complete_widget
        parsed=(${(z)BUFFER})
        printf '%s\000' "${(Q)parsed[2]}" "$PWD" >> "$CJ_TEST_RESULTS"
    done
    exit
}
zle -N zle-line-init
test_buffer=''
vared test_buffer
"#,
    );
    fs::write(&script, setup).expect("write zle test script");
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_cj"));
    let mut paths = vec![binary.parent().unwrap().to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let output = Command::new("zsh")
        .args([
            "-fc",
            r#"zmodload zsh/zpty || exit 1
zpty -b cj_test zsh -fi "$CJ_TEST_SCRIPT" || exit 1
for attempt in {1..200}; do
    while zpty -r cj_test output; do print -rn -- "$output"; done
    zpty -t cj_test || break
    sleep 0.05
done
timed_out=0
zpty -t cj_test && timed_out=1
while zpty -r cj_test output; do print -rn -- "$output"; done
zpty -d cj_test
exit $timed_out"#,
        ])
        .current_dir(&fixture.git.main_nested)
        .env("PATH", env::join_paths(paths).expect("join PATH"))
        .env("TERM", "xterm")
        .env("CJ_TEST_SCRIPT", &script)
        .env("CJ_TEST_RESULTS", &results)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .expect("run real zsh completion");
    assert_success(&output);
    let results = fs::read(results).expect("completion widget produced results");
    let fields = nul_strings(&results);
    assert_eq!(fields.len(), 9);
    for result in fields.chunks_exact(3) {
        assert_eq!(result[0], "2", "real compadd must accept both worktrees");
        let destination = Path::new(result[1]);
        assert!(
            destination == fixture.git.main || destination == fixture.git.linked,
            "completion must insert one quoted worktree path, got {destination:?}"
        );
        assert_eq!(result[2], fixture.git.main_nested.to_str().unwrap());
    }
    assert!(!fixture.args.exists(), "Tab must not run cj's fzf picker");
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

#[test]
fn generated_wrappers_navigate_down_with_custom_tickers() {
    let temp = TempDir::new("shell-navigation");
    let root = temp.path().join("navigation root's path");
    let a = root.join("a");
    let b = a.join("b");
    let leaf = b.join("c");
    let unrelated = a.join("other");
    let shadow_a = root.join("shadow/a");
    let shadow_leaf = shadow_a.join("b/c");
    let shadow_down = shadow_a.join("d");
    for directory in [&leaf, &unrelated, &shadow_leaf, &shadow_down] {
        fs::create_dir_all(directory).expect("create navigation directory");
    }
    let config = temp.path().join("navigation config's.toml");
    fs::write(
        &config,
        "[behavior]\ndefault = \"builtin\"\n\n[tickers]\nnavigate_up = \"u\"\nnavigate_down = \"d\"\n",
    )
    .expect("write navigation config");
    let binary_dir = temp.path().join("navigation binary dir");
    fs::create_dir_all(&binary_dir).expect("create binary directory");
    symlink(env!("CARGO_BIN_EXE_cj"), binary_dir.join("cj")).expect("link cj binary");
    let mut paths = vec![binary_dir];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(paths).expect("join PATH");
    let mut exercised = 0;

    for shell in ["bash", "zsh"] {
        let init = cj(&leaf, temp.path())
            .arg("-C")
            .arg(&config)
            .args(["init", shell])
            .output()
            .expect("render navigation setup");
        assert_success(&init);

        let script = concat!(
            "eval \"$1\"; ",
            "cd uu; printf '%s\\000' \"$PWD\"; ",
            "cd d; printf '%s\\000' \"$PWD\"; ",
            "cd d; printf '%s\\000' \"$PWD\"; ",
            "cd uu; cd dd; printf '%s\\000' \"$PWD\"; ",
            "cd -Z uu; cd -Z dd; printf '%s\\000' \"$PWD\"; ",
            "cd -r \"$2\"; printf '%s\\000' \"$PWD\""
        );
        let mut command = shell_with_setup(shell, &init.stdout, script);
        command
            .arg(&unrelated)
            .current_dir(&leaf)
            .env("PATH", &path);
        let output = match command.output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("cannot run {shell}: {error}"),
        };
        exercised += 1;
        assert_success(&output);
        assert!(output.stderr.is_empty());
        assert_eq!(
            nul_strings(&output.stdout),
            [
                a.to_str().unwrap(),
                b.to_str().unwrap(),
                leaf.to_str().unwrap(),
                leaf.to_str().unwrap(),
                leaf.to_str().unwrap(),
                unrelated.to_str().unwrap()
            ]
        );

        let mut no_history = shell_with_setup(shell, &init.stdout, "eval \"$1\"; cd d");
        no_history.current_dir(&leaf).env("PATH", &path);
        let no_history = no_history.output().expect("run available shell");
        assert_eq!(no_history.status.code(), Some(2));
        assert!(no_history.stdout.is_empty());
        assert_eq!(no_history.stderr, b"cj: no remembered downward route\n");

        let clear_script = "eval \"$1\"; cd uu; cd other; builtin cd ..; cd d";
        let mut cleared = shell_with_setup(shell, &init.stdout, clear_script);
        cleared.current_dir(&leaf).env("PATH", &path);
        let cleared = cleared.output().expect("run available shell");
        assert_eq!(cleared.status.code(), Some(2));
        assert!(cleared.stdout.is_empty());
        assert_eq!(cleared.stderr, b"cj: no remembered downward route\n");

        let shadow_script = "eval \"$1\"; cd uu; cd d; printf '%s\\n' \"$PWD\"";
        let mut shadowed = shell_with_setup(shell, &init.stdout, shadow_script);
        shadowed.current_dir(&shadow_leaf).env("PATH", &path);
        let shadowed = shadowed.output().expect("run available shell");
        assert_success(&shadowed);
        assert_eq!(shadowed.stdout, path_output(&shadow_down));
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
            .arg("-jw")
            .env("CJ_FAKE_ARGS", &self.args)
            .env("CJ_FAKE_STDIN", &self.stdin)
            .env("CJ_FAKE_MODE", mode)
            .env("CJ_FAKE_SELECTION", "1");
        command
    }
}

fn shell_command(shell: &str, setup: &[u8], script: &str) -> Command {
    let mut command = shell_with_setup(shell, setup, script);
    command.args(["two words", "quo'te"]);
    command
}

fn shell_with_setup(shell: &str, setup: &[u8], script: &str) -> Command {
    let mut command = Command::new(shell);
    if shell == "bash" {
        command.args(["--noprofile", "--norc"]);
    } else {
        command.arg("-f");
    }
    command
        .args(["-c", script, "_"])
        .arg(OsStr::from_bytes(setup))
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR");
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
