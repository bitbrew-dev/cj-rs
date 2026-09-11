#![cfg(unix)]

mod support;

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use support::{TempDir, assert_success, cj, write_executable};

#[test]
fn command_completion_hides_cj_metadata_without_hiding_commands_or_changing_styles() {
    if Command::new("zsh").arg("--version").output().is_err() {
        assert!(env::var_os("CJ_REQUIRE_SHELLS").is_none());
        return;
    }
    let temp = TempDir::new("zsh-command-completion");
    let config = temp.path().join("config.toml");
    fs::write(&config, "").unwrap();
    let source = temp.path().join("init.zsh");
    let init = cj(temp.path(), temp.path())
        .arg("-C")
        .arg(config)
        .args(["init", "zsh"])
        .output()
        .unwrap();
    assert_success(&init);
    fs::write(&source, init.stdout).unwrap();
    let bin = temp.path().join("bin");
    write_executable(&bin.join("_cj_external"), ":");
    let mut paths = vec![
        bin,
        Path::new(env!("CARGO_BIN_EXE_cj"))
            .parent()
            .unwrap()
            .to_owned(),
    ];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let script = temp.path().join("completion.zsh");
    fs::write(
        &script,
        r#"zstyle ':completion:*' completer _complete _ignored
zstyle ':completion:*:functions' ignored-patterns '_user_ignored'
_user_visible_function() { :; }
_user_ignored() { :; }
_user_visible_parameter=value
_user_command_completion() {
    # Hiding completion metadata must not disable real helpers or variables.
    _cj_is_repeated aa a || exit 10
    [[ $_cj_navigate_down == v ]] || exit 11
    (( _test_delegated++ ))
    _autocd "$@"
}
_test_styles=$(zstyle -L)
if [[ $CJ_ORDER == before ]]; then source "$CJ_SOURCE"; fi
autoload -Uz compinit
compinit -i -D
compdef _user_command_completion -command-
if [[ $CJ_ORDER == before ]]; then
    # The installed precmd hook handles compinit after sourcing.
    for hook in $precmd_functions; do "$hook"; done
else
    source "$CJ_SOURCE"
fi
source "$CJ_SOURCE"
source "$CJ_SOURCE"
[[ $_cj_command_completion_fallback == _user_command_completion ]] || exit 12
[[ "$(zstyle -L)" == "$_test_styles" ]] || exit 13
compadd() {
    local -a added
    builtin compadd -A added "$@"
    _test_matches+=("${added[@]}")
    builtin compadd "$@"
}
_test_complete() {
    compstate[vared]=''
    compstate[context]=command
    _main_complete
}
zle -C _test_complete_widget complete-word _test_complete
zle-line-init() {
    local prefix
    for prefix in _cj_ _cj_hist _user_visible _user_ignored pri; do
        _test_matches=()
        BUFFER=$prefix; CURSOR=$#BUFFER
        zle _test_complete_widget
        printf '%s\0' "$prefix" "${(j:,:)_test_matches}" >> "$CJ_RESULTS"
    done
    (( _test_delegated >= 5 )) || exit 14
    (( $+functions[_cj_is_repeated] && $+parameters[_cj_navigate_down] )) || exit 15
    [[ "$(zstyle -L)" == "$_test_styles" ]] || exit 16
    print -r -- ok >> "$CJ_DONE"
    exit
}
zle -N zle-line-init
_test_buffer=''; vared _test_buffer
"#,
    )
    .unwrap();
    for order in ["before", "after"] {
        let results = temp.path().join(format!("results-{order}"));
        let done = temp.path().join(format!("done-{order}"));
        let output = Command::new("zsh")
            .args([
                "-fc",
                r#"zmodload zsh/zpty || exit 1
zpty -b completion zsh -fi "$CJ_SCRIPT" || exit 1
for attempt in {1..200}; do
    while zpty -r completion output; do print -rn -- "$output"; done
    zpty -t completion || break
    sleep 0.05
done
zpty -t completion && timed_out=1 || timed_out=0
zpty -d completion
exit $timed_out"#,
            ])
            .current_dir(temp.path())
            .env("PATH", env::join_paths(&paths).unwrap())
            .env("TERM", "xterm")
            .env("CJ_ORDER", order)
            .env("CJ_SOURCE", &source)
            .env("CJ_SCRIPT", &script)
            .env("CJ_RESULTS", &results)
            .env("CJ_DONE", &done)
            .output()
            .unwrap();
        assert_success(&output);
        assert!(
            done.exists(),
            "{order}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let bytes = fs::read(results).unwrap();
        let fields: Vec<_> = bytes.split(|byte| *byte == 0).collect();
        let rows: Vec<_> = fields[..fields.len() - 1].chunks_exact(2).collect();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0][1], b"_cj_external");
        assert!(rows[1][1].is_empty(), "_ignored restored internal helpers");
        let normal = String::from_utf8_lossy(rows[2][1]);
        assert!(normal.contains("_user_visible_function"), "{normal}");
        assert!(normal.contains("_user_visible_parameter"), "{normal}");
        assert!(String::from_utf8_lossy(rows[3][1]).contains("_user_ignored"));
        assert!(String::from_utf8_lossy(rows[4][1]).contains("print"));
    }
}
