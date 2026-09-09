//! Foreground line-editor widgets for ordered directory completion.
use crate::cli::Shell;
use crate::config::{Config, KeyBinding, KeyBindingBehavior};

pub fn render(shell: Shell, chord: Option<KeyBinding>, config: &Config) -> String {
    let cleanup = cleanup(shell);
    let Some(chord @ (KeyBinding::CtrlO | KeyBinding::AltO)) = chord else {
        return cleanup.into();
    };
    if config.key_binding_behaviors().is_empty() || shell == Shell::Nu {
        return cleanup.into();
    }
    let history = config
        .key_binding_behaviors()
        .contains(&KeyBindingBehavior::Cj);
    let helper = if history {
        crate::key_history::render(shell)
    } else {
        ""
    };
    let widget = match shell {
        Shell::Bash | Shell::Zsh => render_posix(shell, chord, config, history),
        Shell::Pwsh => render_powershell(chord, config, history),
        Shell::Nu => unreachable!(),
    };
    format!("{cleanup}\n{helper}\n{widget}")
}

fn cleanup(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => {
            r#"if [[ -n "${_cj_bound_key-}" ]]; then
    while IFS= read -r _cj_existing_binding; do
        if [[ "$_cj_existing_binding" == "\"$_cj_bound_key\" \"${_cj_bound_widget-}\"" || "$_cj_existing_binding" == "\"$_cj_bound_key\": \"${_cj_bound_widget-}\"" ]]; then
            \builtin bind -r "$_cj_bound_key" 2>/dev/null || :
        fi
    done < <(\builtin bind -X 2>/dev/null)
fi
unset _cj_bound_key _cj_bound_widget _cj_existing_binding"#
        }
        Shell::Zsh => {
            r#"if [[ -n "${_cj_bound_key-}" && "$(\builtin bindkey "$_cj_bound_key" 2>/dev/null)" == "\"$_cj_bound_key\" ${_cj_bound_widget-}" ]]; then
    \builtin bindkey -r "$_cj_bound_key"
fi
unset _cj_bound_key _cj_bound_widget"#
        }
        Shell::Pwsh => {
            r#"$__cj_previous_chord = Get-Variable -Name __cj_bound_chord -Scope Global -ValueOnly -ErrorAction SilentlyContinue
if ($__cj_previous_chord -and (Get-Command Get-PSReadLineKeyHandler -ErrorAction SilentlyContinue)) {
    $__cj_previous_handler = Get-PSReadLineKeyHandler -Chord $__cj_previous_chord
    if ($__cj_previous_handler.Function -eq 'cj directory' -and $__cj_previous_handler.Description -eq 'cj managed directory completion') {
        Remove-PSReadLineKeyHandler -Chord $__cj_previous_chord
    }
}
Remove-Variable -Name __cj_bound_chord -Scope Global -ErrorAction SilentlyContinue
Remove-Variable -Name __cj_previous_chord, __cj_previous_handler -ErrorAction SilentlyContinue"#
        }
        Shell::Nu => "",
    }
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn render_posix(shell: Shell, chord: KeyBinding, config: &Config, history: bool) -> String {
    let behaviors = config
        .key_binding_behaviors()
        .iter()
        .map(|behavior| match behavior {
            KeyBindingBehavior::Zoxide => "zoxide",
            KeyBindingBehavior::Cj => "cj",
        })
        .collect::<Vec<_>>()
        .join(" ");
    let zoxide = quote(&config.programs.zoxide.to_string_lossy());
    let fzf = quote(&config.programs.fzf.to_string_lossy());
    let read_buffer = if shell == Shell::Bash {
        "local _cj_buffer=\"${READLINE_LINE-}\" _cj_cursor=\"${READLINE_POINT-0}\"\n    if (( BASH_VERSINFO[0] < 5 )); then local LC_ALL=C; fi"
    } else {
        "emulate -L zsh\n    local _cj_buffer=\"$BUFFER\" _cj_cursor=\"$CURSOR\""
    };
    let apply = if shell == Shell::Bash {
        "READLINE_LINE=\"$_cj_key_buffer\" READLINE_POINT=\"$_cj_key_cursor\""
    } else {
        "BUFFER=\"$_cj_key_buffer\" CURSOR=\"$_cj_key_cursor\""
    };
    let reset = if history {
        "_cj_key_history_reset"
    } else {
        ":"
    };
    let resume = if history {
        format!(
            r#"if [[ -n "${{_cj_key_history_next-}}" && "$_cj_buffer" == "${{_cj_key_history_last-}}" && "$_cj_cursor" == "${{_cj_key_history_last_cursor-}}" ]]; then
        _cj_key_history "$_cj_buffer" "$_cj_cursor"
        {apply}
        return 0
    fi
    _cj_key_history_reset"#
        )
    } else {
        String::new()
    };
    // A sentinel prevents command substitution from stripping newlines in paths.
    let body = format!(
        r#"_cj_key_widget() {{
    {read_buffer}
    {resume}
    local _cj_behavior _cj_reply _cj_status _cj_target _cj_quoted _cj_quote _cj_separator _cj_missing='' _cj_empty='no matching directories'
    for _cj_behavior in {behaviors}; do
        case "$_cj_behavior" in
            cj)
                if _cj_key_history "$_cj_buffer" "$_cj_cursor"; then
                    {apply}
                    return 0
                fi
                _cj_empty='no directory history'
                ;;
            zoxide)
                if [[ -n "${{ZSH_VERSION-}}" ]]; then \builtin zle -I 2>/dev/null || :; fi
                if _cj_reply="$(\command cj --internal-key-binding-zoxide {zoxide} {fzf}; _cj_status=$?; printf '.'; exit "$_cj_status")"; then
                    _cj_status=0
                else
                    _cj_status=$?
                fi
                _cj_reply="${{_cj_reply%.}}"
                if (( _cj_status != 0 )); then
                    {reset}
                    return 0
                fi
                case "$_cj_reply" in
                    selected$'\n'*)
                        _cj_target="${{_cj_reply#*$'\n'}}"
                        {reset}
                        [[ "$_cj_target" != -* ]] || _cj_target="./$_cj_target"
                        _cj_quote="'\\''"
                        _cj_quoted="'${{_cj_target//\'/$_cj_quote}}'"
                        _cj_separator=''
                        [[ -z "$_cj_buffer" || "$_cj_buffer" == *[[:space:]] ]] || _cj_separator=' '
                        _cj_key_buffer="$_cj_buffer$_cj_separator$_cj_quoted"
                        _cj_key_cursor=${{#_cj_key_buffer}}
                        {apply}
                        return 0
                        ;;
                    unavailable$'\n'*) _cj_missing="${{_cj_reply#*$'\n'}}" ;;
                    empty$'\n') ;;
                    cancelled$'\n')
                        {reset}
                        return 0
                        ;;
                    *)
                        printf '%s\n' 'cj: invalid interactive zoxide response' >&2
                        {reset}
                        return 0
                        ;;
                esac
                ;;
        esac
    done
    printf 'cj: %s\n' "${{_cj_missing:-$_cj_empty}}" >&2
    return 0
}}"#
    );
    let binding = match (shell, chord) {
        (Shell::Bash, KeyBinding::CtrlO) => r#"\builtin bind -x '"\C-o":_cj_key_widget'"#,
        (Shell::Bash, _) => r#"\builtin bind -x '"\eo":_cj_key_widget'"#,
        (Shell::Zsh, KeyBinding::CtrlO) => {
            "\\builtin zle -N _cj_key_widget\n\\builtin bindkey '^O' _cj_key_widget"
        }
        (Shell::Zsh, _) => {
            "\\builtin zle -N _cj_key_widget\n\\builtin bindkey '^[o' _cj_key_widget"
        }
        _ => unreachable!(),
    };
    let key = match (shell, chord) {
        (Shell::Bash, KeyBinding::CtrlO) => r"\C-o",
        (Shell::Bash, _) => r"\eo",
        (Shell::Zsh, KeyBinding::CtrlO) => "^O",
        (Shell::Zsh, _) => "^[o",
        _ => unreachable!(),
    };
    let binding = format!("{binding}\n_cj_bound_key='{key}'\n_cj_bound_widget=_cj_key_widget");
    if shell == Shell::Bash {
        format!("{body}\nif (( BASH_VERSINFO[0] >= 4 )); then\n    {binding}\nfi")
    } else {
        format!("{body}\n{binding}")
    }
}

fn render_powershell(chord: KeyBinding, config: &Config, history: bool) -> String {
    let quote = |value: &str| format!("'{}'", value.replace('\'', "''"));
    let zoxide = quote(&config.programs.zoxide.to_string_lossy());
    let fzf = quote(&config.programs.fzf.to_string_lossy());
    let behaviors = config
        .key_binding_behaviors()
        .iter()
        .map(|behavior| match behavior {
            KeyBindingBehavior::Zoxide => "'zoxide'",
            KeyBindingBehavior::Cj => "'cj'",
        })
        .collect::<Vec<_>>()
        .join(", ");
    let reset = if history { "Reset-CjKeyHistory" } else { "" };
    let resume = if history {
        r#"$cycle = $global:__cj_key_history_cycle
    if ($null -ne $cycle -and $buffer -ceq $cycle.LastBuffer -and $cursor -eq $cycle.LastCursor) {
        $result = Invoke-CjKeyHistory -Buffer $buffer -Cursor $cursor
        [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $buffer.Length, $result.Buffer)
        [Microsoft.PowerShell.PSConsoleReadLine]::SetCursorPosition($result.Cursor)
        return
    }
    Reset-CjKeyHistory"#
    } else {
        ""
    };
    let chord = if chord == KeyBinding::CtrlO {
        "Ctrl+o"
    } else {
        "Alt+o"
    };
    format!(
        r#"function global:Invoke-CjKeyWidget {{
    $buffer = ''; $cursor = 0
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$buffer, [ref]$cursor)
    {resume}
    $missing = $null
    $empty = 'no matching directories'
    foreach ($behavior in @({behaviors})) {{
        if ($behavior -eq 'cj') {{
            $result = Invoke-CjKeyHistory -Buffer $buffer -Cursor $cursor
            if ($null -ne $result) {{
                [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $buffer.Length, $result.Buffer)
                [Microsoft.PowerShell.PSConsoleReadLine]::SetCursorPosition($result.Cursor)
                return
            }}
            $empty = 'no directory history'
            continue
        }}
        # PowerShell's native pipeline captures stderr even inside PSReadLine.
        # Redirect only the private stdout protocol; leave the picker on the tty.
        $process = [System.Diagnostics.Process]::new()
        try {{
            $process.StartInfo.FileName = $script:__cj_executable.Path
            $process.StartInfo.UseShellExecute = $false
            $process.StartInfo.RedirectStandardOutput = $true
            $process.StartInfo.StandardOutputEncoding = [System.Text.UTF8Encoding]::new($false)
            $process.StartInfo.WorkingDirectory = (Get-Location).ProviderPath
            foreach ($argument in @('--internal-key-binding-zoxide', {zoxide}, {fzf})) {{
                $process.StartInfo.ArgumentList.Add($argument)
            }}
            [void]$process.Start()
            $raw = $process.StandardOutput.ReadToEnd()
            $process.WaitForExit()
            $status = $process.ExitCode
        }} catch {{
            [Console]::Error.WriteLine("cj: cannot launch interactive zoxide query: $_")
            {reset}; return
        }} finally {{
            $process.Dispose()
        }}
        if ($status -ne 0) {{ [Console]::Error.WriteLine("cj: interactive zoxide query failed (exit $status)"); {reset}; return }}
        $reply = $raw -split "`n", 2
        if ($reply.Count -ne 2) {{ [Console]::Error.WriteLine('cj: invalid interactive zoxide response'); {reset}; return }}
        switch ($reply[0]) {{
            'selected' {{
                if ($reply[1].Length -eq 0) {{ [Console]::Error.WriteLine('cj: invalid interactive zoxide path'); {reset}; return }}
                {reset}
                $separator = if ($buffer.Length -eq 0 -or [char]::IsWhiteSpace($buffer[$buffer.Length - 1])) {{ '' }} else {{ ' ' }}
                $target = [string]$reply[1]
                if ($target.StartsWith('-')) {{ $target = './' + $target }}
                $quoted = [System.Management.Automation.Language.CodeGeneration]::EscapeSingleQuotedStringContent($target)
                $updated = $buffer + $separator + "'" + $quoted + "'"
                [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $buffer.Length, $updated)
                [Microsoft.PowerShell.PSConsoleReadLine]::SetCursorPosition($updated.Length)
                return
            }}
            'unavailable' {{ $missing = $reply[1] }}
            'empty' {{ }}
            'cancelled' {{ {reset}; return }}
            default {{ [Console]::Error.WriteLine('cj: invalid interactive zoxide response'); {reset}; return }}
        }}
    }}
    if ($missing) {{ [Console]::Error.WriteLine("cj: $missing") }} else {{ [Console]::Error.WriteLine("cj: $empty") }}
}}
if (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue) {{
    Set-PSReadLineKeyHandler -Chord '{chord}' -BriefDescription 'cj directory' -Description 'cj managed directory completion' -ScriptBlock {{ Invoke-CjKeyWidget }}
    $global:__cj_bound_chord = '{chord}'
}}"#
    )
}
