use std::path::Path;

use crate::cli::Shell;
use crate::config::{Config, KeyBinding};

pub fn render(
    shell: Shell,
    config_path: Option<&Path>,
    binding: Option<KeyBinding>,
    config: &Config,
) -> Result<String, String> {
    match shell {
        Shell::Bash | Shell::Zsh => render_posix(shell, config_path, binding, config),
        Shell::Nu => render_nu(config_path, config),
        Shell::Pwsh => render_powershell(config_path, binding, config),
    }
}

fn render_posix(
    shell: Shell,
    config_path: Option<&Path>,
    binding: Option<KeyBinding>,
    settings: &Config,
) -> Result<String, String> {
    let config = config_path
        .map(absolute)
        .transpose()?
        .map(|path| format!(" -C {}", quote(&path.to_string_lossy())))
        .unwrap_or_default();
    let builtin = "builtin";
    let navigate_up = quote(&settings.tickers.navigate_up);
    let navigate_down = quote(&settings.tickers.navigate_down);
    let destinations = crate::completions::destinations(settings)
        .iter()
        .map(|value| quote(value))
        .collect::<Vec<_>>()
        .join(" ");

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
    local -a _cj_args

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
        -jw|--jump-worktree)
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
    case "${{1-}}" in
        -z|--zoxide|-Z|--no-zoxide|-jw|--jump-worktree) _cj_args=("$@") ;;
        *) _cj_args=(-- "$@") ;;
    esac
    _cj_before="$(\builtin pwd -P)" || return
    target="$(CJ_INTERNAL_DOWN_ROUTE="${{_cj_down_route-}}" \command cj{config} "${{_cj_args[@]}}")"
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
}}

function _cj_complete_cd() {{
    local current target _cj_status candidate
    local -a destinations
    destinations=({destinations})

    if [[ -n "${{BASH_VERSION-}}" ]]; then
        current="${{COMP_WORDS[COMP_CWORD]}}"
        if (( COMP_CWORD == 1 )) && [[ "$current" == -jw || "$current" == --jump-worktree ]]; then
            target="$(\command cj{config} --jump-worktree)"
            _cj_status=$?
            (( _cj_status == 0 )) || return "$_cj_status"
            if [[ -z "$target" ]]; then
                COMPREPLY=("$current")
                compopt -o nospace 2>/dev/null || :
                return 0
            fi
            COMPREPLY=("$target")
            compopt -o filenames 2>/dev/null || :
            return 0
        fi

        if declare -F _cd >/dev/null; then
            _cd "$@"
        else
            COMPREPLY=()
            while IFS= read -r candidate; do COMPREPLY+=("$candidate"); done < <(compgen -d -- "$current")
        fi
        for candidate in "${{destinations[@]}}"; do
            [[ "$candidate" == "$current"* ]] && COMPREPLY+=("$candidate")
        done
        compopt -o filenames 2>/dev/null || :
        return 0
    fi

    current="${{words[CURRENT]}}"
    if (( CURRENT == 2 )) && [[ "$current" == -jw || "$current" == --jump-worktree ]]; then
        target="$(\command cj{config} --jump-worktree)"
        _cj_status=$?
        (( _cj_status == 0 )) || return "$_cj_status"
        [[ -n "$target" ]] || return 1
        compadd -f -- "$target"
        return
    fi

    compadd -X 'cj destination' -- "${{destinations[@]}}"
    "${{_cj_cd_completion_fallback:-_cd}}" "$@"
}}

if [[ -n "${{BASH_VERSION-}}" ]]; then
    complete -F _cj_complete_cd cd
elif (( $+functions[compdef] )); then
    if [[ "${{_comps[cd]-}}" != _cj_complete_cd ]]; then
        _cj_cd_completion_fallback="${{_comps[cd]-_cd}}"
    fi
    compdef _cj_complete_cd cd
fi"#
    );
    Ok(match binding {
        Some(KeyBinding::CtrlO) => format!("{wrapper}\n\n{}", render_binding(shell, &config, true)),
        Some(KeyBinding::AltO) => format!("{wrapper}\n\n{}", render_binding(shell, &config, false)),
        Some(KeyBinding::None) | None => wrapper,
    })
}

fn render_binding(shell: Shell, config: &str, control: bool) -> String {
    let command = format!("\\command cj{config} --jump-worktree");
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
        Shell::Nu | Shell::Pwsh => unreachable!("bindings are POSIX-shell only"),
    }
}

fn render_nu(config_path: Option<&Path>, settings: &Config) -> Result<String, String> {
    let config = config_path
        .map(absolute)
        .transpose()?
        .map(|path| format!("[-C {}]", quote_nu(&path.to_string_lossy())))
        .unwrap_or_else(|| "[]".into());
    let navigate_up = quote_nu(&settings.tickers.navigate_up);
    let navigate_down = quote_nu(&settings.tickers.navigate_down);
    let destinations = crate::completions::destinations(settings)
        .iter()
        .map(|value| {
            format!(
                "{{ value: {}, description: 'cj destination' }}",
                quote_nu(value)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        r#"export-env {{
    $env.CJ_INTERNAL_DOWN_ROUTE = ($env.CJ_INTERNAL_DOWN_ROUTE? | default '')
}}

def _cj-is-repeated [value: string, ticker: string] {{
    ($value | str length) > 0 and (($value | split chars | all {{ |char| $char == $ticker }}))
}}

def _cj-complete-cd [spans: list<string>] {{
    let current = ($spans | last)
    let configured = ([{destinations}] | where value starts-with $current)
    let directories = ($current | commandline complete --detailed --type directory | each {{ |entry|
        {{ value: $entry.value, description: 'directory' }}
    }})
    $configured | append $directories
}}

@complete '_cj-complete-cd'
export def --env --wrapped __cj_cd [...args: string] {{
    let config = {config}
    let navigate_up = {navigate_up}
    let navigate_down = {navigate_down}

    if ($args | is-empty) {{
        cd $nu.home-path
        $env.CJ_INTERNAL_DOWN_ROUTE = ''
        return
    }}

    if (($args.0 == '-r') or ($args.0 == '--raw')) {{
        if ($args | length) != 2 {{
            error make {{ msg: 'cj: --raw requires exactly one target' }}
        }}
        cd $args.1
        $env.CJ_INTERNAL_DOWN_ROUTE = ''
        return
    }}

    if (($args.0 == '-P') or ($args.0 == '--physical')) {{
        if ($args | length) > 2 {{ error make {{ msg: 'cj: --physical accepts at most one target' }} }}
        if ($args | length) == 1 {{ cd --physical }} else {{ cd --physical $args.1 }}
        $env.CJ_INTERNAL_DOWN_ROUTE = ''
        return
    }}

    if (($args | length) == 1) and (($args.0 == '-h') or ($args.0 == '--help')) {{
        help cd
        return
    }}

    if (($args | length) == 1) and (not ($args.0 | str starts-with '-')) {{
        let direct = (try {{ cd $args.0; true }} catch {{ false }})
        if $direct {{
            $env.CJ_INTERNAL_DOWN_ROUTE = ''
            return
        }}
    }} else if (($args | length) == 2) and (($args.0 == '-Z') or ($args.0 == '--no-zoxide')) {{
        let direct = (try {{ cd $args.1; true }} catch {{ false }})
        if $direct {{
            $env.CJ_INTERNAL_DOWN_ROUTE = ''
            return
        }}
    }} else if ($args.0 | str starts-with '-') and (not ($args.0 in ['-z' '--zoxide' '-Z' '--no-zoxide' '-jw' '--jump-worktree'])) {{
        if ($args | length) != 1 {{ error make {{ msg: 'cj: Nushell cd accepts one target' }} }}
        cd $args.0
        $env.CJ_INTERNAL_DOWN_ROUTE = ''
        return
    }}

    let nav_arg = if (($args | length) == 1) {{ $args.0 }} else if (($args | length) == 2) and (($args.0 == '-Z') or ($args.0 == '--no-zoxide')) {{ $args.1 }} else {{ '' }}
    let before = ($env.PWD | path expand)
    let invoke_args = if ($args.0 in ['-z' '--zoxide' '-Z' '--no-zoxide' '-jw' '--jump-worktree']) {{ $args }} else {{ ['--'] | append $args }}
    let result = with-env {{ CJ_INTERNAL_DOWN_ROUTE: $env.CJ_INTERNAL_DOWN_ROUTE }} {{
        ^cj ...$config ...$invoke_args | complete
    }}
    if $result.exit_code != 0 {{
        if not ($result.stderr | is-empty) {{ print --stderr --no-newline $result.stderr }}
        error make {{ msg: $'cj exited with status ($result.exit_code)' }}
    }}
    let target = ($result.stdout | str replace --regex '\r?\n$' '')
    if ($target | is-empty) {{ error make {{ msg: 'cj returned an empty destination' }} }}
    cd $target
    let after = ($env.PWD | path expand)

    if (_cj-is-repeated $nav_arg $navigate_up) and ($after != $before) {{
        let route_is_below = if ($env.CJ_INTERNAL_DOWN_ROUTE | is-empty) {{ false }} else {{
            try {{
                let relative = ($env.CJ_INTERNAL_DOWN_ROUTE | path relative-to $before)
                (($relative | path split | first) != '..')
            }} catch {{ false }}
        }}
        if not $route_is_below {{
            $env.CJ_INTERNAL_DOWN_ROUTE = $before
        }}
    }} else if (_cj-is-repeated $nav_arg $navigate_down) {{
        if $after == $env.CJ_INTERNAL_DOWN_ROUTE {{ $env.CJ_INTERNAL_DOWN_ROUTE = '' }}
    }} else {{
        $env.CJ_INTERNAL_DOWN_ROUTE = ''
    }}
}}

export alias cd = __cj_cd"#
    ))
}

fn render_powershell(
    config_path: Option<&Path>,
    binding: Option<KeyBinding>,
    settings: &Config,
) -> Result<String, String> {
    let config = config_path
        .map(absolute)
        .transpose()?
        .map(|path| format!("@('-C', {})", quote_powershell(&path.to_string_lossy())))
        .unwrap_or_else(|| "@()".into());
    let navigate_up = quote_powershell(&settings.tickers.navigate_up);
    let navigate_down = quote_powershell(&settings.tickers.navigate_down);
    let destinations = crate::completions::destinations(settings)
        .iter()
        .map(|value| quote_powershell(value))
        .collect::<Vec<_>>()
        .join(", ");
    let wrapper = format!(
        r#"$script:__cj_executable = Get-Command cj -CommandType Application -ErrorAction Stop | Select-Object -First 1
$script:__cj_config = {config}
$script:__cj_destinations = @({destinations})
$global:__cj_down_route = $null
$script:__cj_navigate_up = {navigate_up}
$script:__cj_navigate_down = {navigate_down}
$script:__cj_path_comparison = if ($env:OS -eq 'Windows_NT') {{ [System.StringComparison]::OrdinalIgnoreCase }} else {{ [System.StringComparison]::Ordinal }}

function script:Test-CjRepeated {{
    param([string]$Value, [string]$Ticker)
    if ([string]::IsNullOrEmpty($Value) -or [string]::IsNullOrEmpty($Ticker)) {{ return $false }}
    return $Value.Replace($Ticker, '').Length -eq 0
}}

function script:Test-CjDescendant {{
    param([string]$Root, [string]$Candidate)
    if ([string]::IsNullOrEmpty($Root) -or [string]::IsNullOrEmpty($Candidate)) {{ return $false }}
    $relative = [System.IO.Path]::GetRelativePath($Root, $Candidate)
    $parentPrefix = '..' + [System.IO.Path]::DirectorySeparatorChar
    return -not [System.IO.Path]::IsPathRooted($relative) -and $relative -ne '..' -and -not $relative.StartsWith($parentPrefix, $script:__cj_path_comparison)
}}

function script:ConvertTo-CjCompletionText {{
    param([string]$Value)
    return "'" + $Value.Replace("'", "''") + "'"
}}

function script:Complete-CjCdArgument {{
    param([string]$WordToComplete)

    if (($WordToComplete -ceq '-jw') -or ($WordToComplete -ceq '--jump-worktree')) {{
        $configArgs = $script:__cj_config
        $executable = $script:__cj_executable.Path
        $target = @(& $executable @configArgs --jump-worktree)
        if (($LASTEXITCODE -eq 0) -and ($target.Count -eq 1) -and -not [string]::IsNullOrEmpty($target[0])) {{
            $completion = ConvertTo-CjCompletionText $target[0]
            [System.Management.Automation.CompletionResult]::new($completion, $target[0], 'ProviderContainer', $target[0])
        }}
        return
    }}

    foreach ($destination in $script:__cj_destinations) {{
        if ($destination.StartsWith($WordToComplete, [System.StringComparison]::OrdinalIgnoreCase)) {{
            $completion = ConvertTo-CjCompletionText $destination
            [System.Management.Automation.CompletionResult]::new($completion, $destination, 'ParameterValue', 'cj destination')
        }}
    }}
    [System.Management.Automation.CompletionCompleters]::CompleteFilename($WordToComplete) |
        Where-Object ResultType -eq ([System.Management.Automation.CompletionResultType]::ProviderContainer)
}}

Remove-Item Alias:cd -Force -ErrorAction SilentlyContinue
function global:cd {{
    $cjArgs = @($args)
    if ($cjArgs.Count -eq 0) {{
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $HOME -ErrorAction Stop
        $global:__cj_down_route = $null
        return
    }}
    $first = [string]$cjArgs[0]

    if (($first -ceq '-r') -or ($first -ceq '--raw')) {{
        if ($cjArgs.Count -ne 2) {{ throw 'cj: --raw requires exactly one target' }}
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $cjArgs[1] -ErrorAction Stop
        $global:__cj_down_route = $null
        return
    }}

    if (($cjArgs.Count -eq 1) -and (Test-Path -LiteralPath $cjArgs[0] -PathType Container)) {{
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $cjArgs[0] -ErrorAction Stop
        $global:__cj_down_route = $null
        return
    }}
    if (($cjArgs.Count -eq 1) -and ($first -ceq '-')) {{
        Microsoft.PowerShell.Management\Set-Location -Path '-' -ErrorAction Stop
        $global:__cj_down_route = $null
        return
    }}
    if (($first -ceq '-Path') -and ($cjArgs.Count -eq 2)) {{
        Microsoft.PowerShell.Management\Set-Location -Path $cjArgs[1] -ErrorAction Stop
        $global:__cj_down_route = $null
        return
    }}
    if (($first -ceq '-LiteralPath') -and ($cjArgs.Count -eq 2)) {{
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $cjArgs[1] -ErrorAction Stop
        $global:__cj_down_route = $null
        return
    }}
    if ($first.StartsWith('-') -and -not (($first -ceq '-z') -or ($first -ceq '--zoxide') -or ($first -ceq '-Z') -or ($first -ceq '--no-zoxide') -or ($first -ceq '-jw') -or ($first -ceq '--jump-worktree'))) {{ throw "cj: unsupported PowerShell cd option: $first" }}

    $navArg = if ($cjArgs.Count -eq 1) {{ $first }} elseif (($cjArgs.Count -eq 2) -and (($first -ceq '-Z') -or ($first -ceq '--no-zoxide'))) {{ [string]$cjArgs[1] }} else {{ '' }}
    $before = (Microsoft.PowerShell.Management\Get-Location).ProviderPath
    [string[]]$invokeArgs = if (($first -ceq '-z') -or ($first -ceq '--zoxide') -or ($first -ceq '-Z') -or ($first -ceq '--no-zoxide') -or ($first -ceq '-jw') -or ($first -ceq '--jump-worktree')) {{ $cjArgs }} else {{ @('--') + $cjArgs }}
    $hadRoute = Test-Path Env:CJ_INTERNAL_DOWN_ROUTE
    $oldRoute = $env:CJ_INTERNAL_DOWN_ROUTE
    $configArgs = $script:__cj_config
    $executable = $script:__cj_executable.Path
    try {{
        $env:CJ_INTERNAL_DOWN_ROUTE = if ($null -eq $global:__cj_down_route) {{ '' }} else {{ $global:__cj_down_route }}
        $target = @(& $executable @configArgs @invokeArgs)
        $status = $LASTEXITCODE
    }} finally {{
        if ($hadRoute) {{ $env:CJ_INTERNAL_DOWN_ROUTE = $oldRoute }} else {{ Remove-Item Env:CJ_INTERNAL_DOWN_ROUTE -ErrorAction SilentlyContinue }}
    }}
    if ($status -ne 0) {{ throw "cj exited with status $status" }}
    if ($target.Count -ne 1 -or [string]::IsNullOrEmpty($target[0])) {{ throw 'cj returned an invalid destination' }}
    Microsoft.PowerShell.Management\Set-Location -LiteralPath $target[0] -ErrorAction Stop
    $after = (Microsoft.PowerShell.Management\Get-Location).ProviderPath

    if ((Test-CjRepeated $navArg $script:__cj_navigate_up) -and -not $after.Equals($before, $script:__cj_path_comparison)) {{
        if (($null -eq $global:__cj_down_route) -or -not (Test-CjDescendant $before $global:__cj_down_route)) {{
            $global:__cj_down_route = $before
        }}
    }} elseif (Test-CjRepeated $navArg $script:__cj_navigate_down) {{
        if (($null -ne $global:__cj_down_route) -and $after.Equals($global:__cj_down_route, $script:__cj_path_comparison)) {{ $global:__cj_down_route = $null }}
    }} else {{
        $global:__cj_down_route = $null
    }}
}}

Register-ArgumentCompleter -Native -CommandName cd -ScriptBlock {{
    param($wordToComplete, $commandAst, $cursorPosition)
    Complete-CjCdArgument $wordToComplete
}}"#
    );
    Ok(match binding {
        Some(KeyBinding::CtrlO) => {
            format!("{wrapper}\n\n{}", render_powershell_binding("Ctrl+o"))
        }
        Some(KeyBinding::AltO) => {
            format!("{wrapper}\n\n{}", render_powershell_binding("Alt+o"))
        }
        Some(KeyBinding::None) | None => wrapper,
    })
}

fn render_powershell_binding(chord: &str) -> String {
    format!(
        r#"if (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue) {{
    Set-PSReadLineKeyHandler -Chord '{chord}' -BriefDescription 'cj worktree' -ScriptBlock {{
        $configArgs = $script:__cj_config
        $executable = $script:__cj_executable.Path
        $target = @(& $executable @configArgs --jump-worktree)
        if (($LASTEXITCODE -eq 0) -and ($target.Count -eq 1) -and -not [string]::IsNullOrEmpty($target[0])) {{
            Microsoft.PowerShell.Management\Set-Location -LiteralPath $target[0] -ErrorAction Stop
            $global:__cj_down_route = $null
        }}
    }}
}}"#
    )
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

fn quote_nu(value: &str) -> String {
    for count in 1.. {
        let hashes = "#".repeat(count);
        if !value.contains(&format!("'{hashes}")) {
            return format!("r{hashes}'{value}'{hashes}");
        }
    }
    unreachable!()
}

fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_uses_command_and_builtin_boundaries() {
        let output = render(Shell::Zsh, None, None, &Config::default()).unwrap();
        assert!(output.contains("\\command cj \"${_cj_args[@]}\""));
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
            &Config::default(),
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
            &Config::default(),
        )
        .unwrap();
        assert!(bash.contains(r#"\builtin bind -x '"\C-o":_cj_worktree_widget'"#));
        assert!(bash.contains("\\command cj --jump-worktree"));

        let zsh = render(Shell::Zsh, None, Some(KeyBinding::AltO), &Config::default()).unwrap();
        assert!(zsh.contains("\\builtin bindkey '^[o' _cj_worktree_widget"));
        assert!(zsh.contains("\\builtin zle reset-prompt"));

        let powershell = render(
            Shell::Pwsh,
            None,
            Some(KeyBinding::CtrlO),
            &Config::default(),
        )
        .unwrap();
        assert!(powershell.contains("Set-PSReadLineKeyHandler -Chord 'Ctrl+o'"));
        assert!(powershell.contains("Microsoft.PowerShell.Management\\Set-Location"));
        assert!(!powershell.contains("Invoke-Expression"));
    }

    #[test]
    fn wrappers_forward_jump_worktree_to_cj() {
        let bash = render(Shell::Bash, None, None, &Config::default()).unwrap();
        assert!(bash.contains("-jw|--jump-worktree)"));
        assert!(bash.contains("-jw|--jump-worktree) _cj_args=(\"$@\")"));
        assert!(!bash.contains("pick-worktree"));

        let nu = render(Shell::Nu, None, None, &Config::default()).unwrap();
        assert!(nu.contains("'-jw' '--jump-worktree'"));
        assert!(nu.contains("^cj ...$config ...$invoke_args | complete"));
        assert!(!nu.contains("pick-worktree"));

        let powershell = render(Shell::Pwsh, None, None, &Config::default()).unwrap();
        assert!(powershell.contains("($first -ceq '-jw')"));
        assert!(powershell.contains("($first -ceq '--jump-worktree')"));
        assert!(powershell.contains("[string[]]$invokeArgs"));
        assert!(!powershell.contains("pick-worktree"));
    }

    #[test]
    fn renders_jump_worktree_completion_hooks() {
        let bash = render(Shell::Bash, None, None, &Config::default()).unwrap();
        assert!(bash.contains("(( COMP_CWORD == 1 ))"));
        assert!(bash.contains("complete -F _cj_complete_cd cd"));
        assert!(bash.contains("\\command cj --jump-worktree"));
        assert!(bash.contains("declare -F _cd"));

        let zsh = render(Shell::Zsh, None, None, &Config::default()).unwrap();
        assert!(zsh.contains("(( CURRENT == 2 ))"));
        assert!(zsh.contains("compadd -f -- \"$target\""));
        assert!(zsh.contains("_cj_cd_completion_fallback"));
        assert!(zsh.contains("compdef _cj_complete_cd cd"));

        let powershell = render(Shell::Pwsh, None, None, &Config::default()).unwrap();
        assert!(powershell.contains("function script:Complete-CjCdArgument"));
        assert!(powershell.contains("$cjArgs = @($args)"));
        assert!(
            powershell.contains("Register-ArgumentCompleter -Native -CommandName cd -ScriptBlock")
        );
        assert!(powershell.contains("CompletionCompleters]::CompleteFilename"));
        assert!(!powershell.contains("Set-PSReadLineKeyHandler -Key Tab"));

        let nu = render(Shell::Nu, None, None, &Config::default()).unwrap();
        assert!(!nu.contains("executehostcommand"));
    }

    #[test]
    fn renders_environment_aware_nushell_integration() {
        let output = render(
            Shell::Nu,
            Some(Path::new("it's config.toml")),
            None,
            &Config::default(),
        )
        .unwrap();
        assert!(output.contains("export def --env --wrapped __cj_cd"));
        assert!(output.contains("export alias cd = __cj_cd"));
        assert!(output.contains("^cj ...$config ...$invoke_args | complete"));
        assert!(output.contains("r#'"));
        assert!(!output.contains("eval"));
    }

    #[test]
    fn renders_literal_powershell_integration() {
        let output = render(
            Shell::Pwsh,
            Some(Path::new("it's config.toml")),
            None,
            &Config::default(),
        )
        .unwrap();
        assert!(output.contains("Microsoft.PowerShell.Management\\Set-Location -LiteralPath"));
        assert!(output.contains("Get-Command cj -CommandType Application"));
        assert!(output.contains("it''s config.toml'"));
        assert!(output.contains("-ceq '-z'"));
        assert!(!output.contains("Invoke-Expression"));
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
            let generated = render(shell, None, None, &Config::default()).unwrap();
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

        let generated = render(Shell::Zsh, None, None, &Config::default()).unwrap();
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
