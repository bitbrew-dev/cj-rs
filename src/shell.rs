use std::path::Path;

use crate::cli::Shell;

pub fn render(shell: Shell, config_path: Option<&Path>) -> String {
    let config = config_path
        .map(|path| format!(" -C {}", quote(&path.to_string_lossy())))
        .unwrap_or_default();
    let builtin = match shell {
        Shell::Bash | Shell::Zsh => "builtin",
    };

    format!(
        r#"cd() {{
    local target status

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
    status=$?
    (( status == 0 )) || return "$status"
    [[ -n "$target" ]] || return 1
    \{builtin} cd -- "$target"
}}"#
    )
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_uses_command_and_builtin_boundaries() {
        let output = render(Shell::Zsh, None);
        assert!(output.contains("target=\"$(\\command cj \"$@\")\""));
        assert!(output.contains("\\builtin cd -- \"$target\""));
        assert!(output.contains("-Z|--no-zoxide"));
    }

    #[test]
    fn config_path_is_shell_quoted() {
        let output = render(Shell::Bash, Some(Path::new("it's here/config.toml")));
        assert!(output.contains("-C 'it'\\''s here/config.toml'"));
    }
}
