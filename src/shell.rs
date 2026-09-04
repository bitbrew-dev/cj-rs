use std::path::Path;

use crate::cli::Shell;
use crate::config::{KeyBinding, Tickers};

pub fn render(
    shell: Shell,
    config_path: Option<&Path>,
    binding: Option<KeyBinding>,
    tickers: &Tickers,
) -> Result<String, String> {
    let config = config_path
        .map(absolute)
        .transpose()?
        .map(|path| format!(" -C {}", quote(&path.to_string_lossy())))
        .unwrap_or_default();
    let builtin = match shell {
        Shell::Bash | Shell::Zsh => "builtin",
    };
    let navigate_up = quote(&tickers.navigate_up);
    let navigate_down = quote(&tickers.navigate_down);

    let wrapper = format!(
        r#"_cj_navigate_up={navigate_up}
_cj_navigate_down={navigate_down}
unset _cj_down_route

function _cj_is_repeated() {{
    local value="$1" ticker="$2"
    [[ -n "$value" && -n "$ticker" ]] || return 1
    while [[ -n "$value" ]]; do
        [[ "${{value#"$ticker"}}" != "$value" ]] || return 1
        value="${{value#"$ticker"}}"
    done
    return 0
}}

\builtin unalias cd 2>/dev/null || :
function cd() {{
    local target _cj_status _cj_before _cj_after _cj_nav_arg _cj_nav_mode

    if [[ $# -eq 0 ]]; then
        \{builtin} cd
        _cj_status=$?
        (( _cj_status == 0 )) && unset _cj_down_route
        return "$_cj_status"
    fi

    case "${{1-}}" in
        -r|--raw)
            shift
            if [[ $# -ne 1 ]]; then
                printf '%s\n' 'cj: --raw requires exactly one target' >&2
                return 2
            fi
            \{builtin} cd -- "$1"
            _cj_status=$?
            (( _cj_status == 0 )) && unset _cj_down_route
            return "$_cj_status"
            ;;
        -z|--zoxide)
            _cj_nav_arg=
            ;;
        -Z|--no-zoxide)
            if [[ $# -eq 2 ]]; then
                if \{builtin} cd -- "$2" 2>/dev/null; then
                    unset _cj_down_route
                    return
                fi
                _cj_nav_arg="$2"
            fi
            ;;
        -*)
            \{builtin} cd "$@"
            _cj_status=$?
            (( _cj_status == 0 )) && unset _cj_down_route
            return "$_cj_status"
            ;;
        *)
            if \{builtin} cd "$@" 2>/dev/null; then
                unset _cj_down_route
                return
            fi
            [[ $# -eq 1 ]] && _cj_nav_arg="$1"
            ;;
    esac

    _cj_nav_mode=other
    if _cj_is_repeated "${{_cj_nav_arg-}}" "$_cj_navigate_up"; then
        _cj_nav_mode=up
    elif _cj_is_repeated "${{_cj_nav_arg-}}" "$_cj_navigate_down"; then
        _cj_nav_mode=down
    fi
    _cj_before="$(\builtin pwd -P)" || return
    target="$(CJ_INTERNAL_DOWN_ROUTE="${{_cj_down_route-}}" \command cj{config} "$@")"
    _cj_status=$?
    (( _cj_status == 0 )) || return "$_cj_status"
    [[ -n "$target" ]] || return 1
    \{builtin} cd -- "$target"
    _cj_status=$?
    (( _cj_status == 0 )) || return "$_cj_status"
    _cj_after="$(\builtin pwd -P)" || return

    case "$_cj_nav_mode" in
        up)
            if [[ "$_cj_after" != "$_cj_before" ]]; then
                if [[ -z "${{_cj_down_route-}}" || ( "$_cj_down_route" != "$_cj_before" && "$_cj_down_route" != "$_cj_before"/* ) ]]; then
                    _cj_down_route="$_cj_before"
                fi
            fi
            ;;
        down)
            [[ "$_cj_after" == "${{_cj_down_route-}}" ]] && unset _cj_down_route
            ;;
        *)
            unset _cj_down_route
            ;;
    esac
    return 0
}}"#
    );
    Ok(match binding {
        Some(KeyBinding::CtrlO) => format!("{wrapper}\n\n{}", render_binding(shell, &config, true)),
        Some(KeyBinding::AltO) => format!("{wrapper}\n\n{}", render_binding(shell, &config, false)),
        Some(KeyBinding::None) | None => wrapper,
    })
}

fn render_binding(shell: Shell, config: &str, control: bool) -> String {
    let command = format!("\\command cj{config} --pick-worktree");
    match shell {
        Shell::Bash => {
            let key = if control { r#"\C-o"# } else { r#"\eo"# };
            format!(
                r#"_cj_worktree_widget() {{
    local target
    target="$({command})" || return
    [[ -n "$target" ]] || return
    \builtin cd -- "$target"
    unset _cj_down_route
}}
\builtin bind -x '"{key}":_cj_worktree_widget'"#
            )
        }
        Shell::Zsh => {
            let key = if control { "^O" } else { "^[o" };
            format!(
                r#"_cj_worktree_widget() {{
    local target _cj_status
    target="$({command})"
    _cj_status=$?
    if (( _cj_status == 0 )) && [[ -n "$target" ]]; then
        \builtin cd -- "$target"
        _cj_status=$?
        (( _cj_status == 0 )) && unset _cj_down_route
    fi
    \builtin zle reset-prompt
    return "$_cj_status"
}}
\builtin zle -N _cj_worktree_widget
\builtin bindkey '{key}' _cj_worktree_widget"#
            )
        }
    }
}

fn absolute(path: &Path) -> Result<std::path::PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.into());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|error| format!("cannot resolve config path: {error}"))
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_uses_command_and_builtin_boundaries() {
        let output = render(Shell::Zsh, None, None, &Tickers::default()).unwrap();
        assert!(output.contains("\\command cj \"$@\""));
        assert!(output.contains("\\builtin cd -- \"$target\""));
        assert!(output.contains("-Z|--no-zoxide"));
        assert!(output.contains("\\builtin unalias cd"));
        assert!(output.contains("function cd()"));
        assert!(output.contains("CJ_INTERNAL_DOWN_ROUTE"));
    }

    #[test]
    fn config_path_is_shell_quoted() {
        let output = render(
            Shell::Bash,
            Some(Path::new("it's here/config.toml")),
            None,
            &Tickers::default(),
        )
        .unwrap();
        assert!(output.contains("it'\\''s here/config.toml'"));
        assert!(
            output.contains(
                &std::env::current_dir()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            )
        );
    }

    #[test]
    fn renders_os_binding_for_each_shell() {
        let bash = render(
            Shell::Bash,
            None,
            Some(KeyBinding::CtrlO),
            &Tickers::default(),
        )
        .unwrap();
        assert!(bash.contains(r#"\builtin bind -x '"\C-o":_cj_worktree_widget'"#));
        assert!(bash.contains("\\command cj --pick-worktree"));

        let zsh = render(
            Shell::Zsh,
            None,
            Some(KeyBinding::AltO),
            &Tickers::default(),
        )
        .unwrap();
        assert!(zsh.contains("\\builtin bindkey '^[o' _cj_worktree_widget"));
        assert!(zsh.contains("\\builtin zle reset-prompt"));
    }

    #[cfg(unix)]
    #[test]
    fn setup_replaces_an_existing_cd_alias() {
        for (shell, program, args, prelude) in [
            (
                Shell::Bash,
                "bash",
                &["--noprofile", "--norc", "-c"][..],
                "shopt -s expand_aliases; alias cd=false",
            ),
            (Shell::Zsh, "zsh", &["-f", "-c"][..], "alias cd=false"),
        ] {
            let generated = render(shell, None, None, &Tickers::default()).unwrap();
            let script = format!("set -e; {prelude}; eval \"$1\"; eval 'cd -Z /'; [[ $PWD == / ]]");
            let output = match std::process::Command::new(program)
                .args(args)
                .arg(script)
                .arg("_")
                .arg(generated)
                .output()
            {
                Ok(output) => output,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("cannot run {program}: {error}"),
            };
            assert!(
                output.status.success(),
                "{program}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn zsh_keeps_two_operand_cd_substitution() {
        let base = std::env::temp_dir().join(format!("cj-shell-test-{}", std::process::id()));
        let old = base.join("__cj_from__");
        let new = base.join("__cj_to__");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();

        let generated = render(Shell::Zsh, None, None, &Tickers::default()).unwrap();
        let output = std::process::Command::new("zsh")
            .args([
                "-f",
                "-c",
                "set -e; builtin cd \"$3\"; eval \"$1\"; cd __cj_from__ __cj_to__; [[ $PWD == $2 ]]",
                "_",
            ])
            .arg(generated)
            .arg(&new)
            .arg(&old)
            .current_dir(&old)
            .output();
        if let Ok(output) = output {
            assert!(
                output.status.success(),
                "zsh: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::fs::remove_dir_all(base).unwrap();
    }
}
