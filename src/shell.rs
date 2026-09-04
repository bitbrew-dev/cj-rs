use std::path::Path;

use crate::cli::Shell;
use crate::config::KeyBinding;

pub fn render(shell: Shell, config_path: Option<&Path>, binding: Option<KeyBinding>) -> String {
    let config = config_path
        .map(|path| format!(" -C {}", quote(&path.to_string_lossy())))
        .unwrap_or_default();
    let builtin = match shell {
        Shell::Bash | Shell::Zsh => "builtin",
    };

    let wrapper = format!(
        r#"cd() {{
    local target _cj_status

    if [[ $# -eq 0 ]]; then
        \{builtin} cd
        return
    fi

    case "${{1-}}" in
        -Z|--no-zoxide)
            shift
            \{builtin} cd "$@"
            return
            ;;
        -z|--zoxide)
            ;;
        -*)
            \{builtin} cd "$@"
            return
            ;;
        *)
            if [[ $# -eq 1 ]] && \{builtin} cd -- "$1" 2>/dev/null; then
                return
            fi
            ;;
    esac

    target="$(\command cj{config} "$@")"
    _cj_status=$?
    (( _cj_status == 0 )) || return "$_cj_status"
    [[ -n "$target" ]] || return 1
    \{builtin} cd -- "$target"
}}"#
    );
    match binding {
        Some(KeyBinding::CtrlO) => format!("{wrapper}\n\n{}", render_binding(shell, &config, true)),
        Some(KeyBinding::AltO) => format!("{wrapper}\n\n{}", render_binding(shell, &config, false)),
        Some(KeyBinding::None) | None => wrapper,
    }
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
}}
bind -x '"{key}":_cj_worktree_widget'"#
            )
        }
        Shell::Zsh => {
            let key = if control { "^O" } else { "^[o" };
            format!(
                r#"_cj_worktree_widget() {{
    local target
    target="$({command})" || return
    [[ -n "$target" ]] || return
    \builtin cd -- "$target" || return
    zle reset-prompt
}}
zle -N _cj_worktree_widget
bindkey '{key}' _cj_worktree_widget"#
            )
        }
    }
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_uses_command_and_builtin_boundaries() {
        let output = render(Shell::Zsh, None, None);
        assert!(output.contains("target=\"$(\\command cj \"$@\")\""));
        assert!(output.contains("\\builtin cd -- \"$target\""));
        assert!(output.contains("-Z|--no-zoxide"));
    }

    #[test]
    fn config_path_is_shell_quoted() {
        let output = render(Shell::Bash, Some(Path::new("it's here/config.toml")), None);
        assert!(output.contains("-C 'it'\\''s here/config.toml'"));
    }

    #[test]
    fn renders_os_binding_for_each_shell() {
        let bash = render(Shell::Bash, None, Some(KeyBinding::CtrlO));
        assert!(bash.contains(r#"bind -x '"\C-o":_cj_worktree_widget'"#));
        assert!(bash.contains("\\command cj --pick-worktree"));

        let zsh = render(Shell::Zsh, None, Some(KeyBinding::AltO));
        assert!(zsh.contains("bindkey '^[o' _cj_worktree_widget"));
        assert!(zsh.contains("zle reset-prompt"));
    }
}
