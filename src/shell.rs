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
        Shell::Nu => render_nu(config_path, config).map(|wrapper| {
            format!(
                "{wrapper}\n{}",
                crate::key_bindings::render(shell, binding, config)
            )
        }),
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
_cj_history=()

function _cj_is_repeated() {{
    local value="$1" ticker="$2"
    [[ -n "$value" && -n "$ticker" ]] || return 1
    while [[ -n "$value" ]]; do
        [[ "${{value#"$ticker"}}" != "$value" ]] || return 1
        value="${{value#"$ticker"}}"
    done
    return 0
}}

function _cj_record_move() {{
    local before="$1" after="$2" mode="${{3-other}}" parent physical previous
    [[ "$before" != "$after" ]] || return 0
    if typeset -f _cj_key_history_reset >/dev/null; then _cj_key_history_reset; fi
    _cj_history+=("$before")
    if [[ "$mode" == up ]]; then
        # The native cd resolves .. against its logical path, including symlinks.
        parent="${{OLDPWD%/*}}"
        previous="$before"
        [[ -n "$parent" ]] || parent=/
        while [[ "$parent" != / ]]; do
            physical="$(\builtin cd -- "$parent" && \builtin pwd -P)" || break
            [[ "$physical" != "$after" ]] || break
            if [[ "$physical" != "$previous" ]]; then _cj_history+=("$physical"); fi
            previous="$physical"
            parent="${{parent%/*}}"
            [[ -n "$parent" ]] || parent=/
        done
    fi
    if (( ${{#_cj_history[@]}} > 100 )); then
        _cj_history=("${{_cj_history[@]: -100}}")
    fi
    return 0
}}

function _cj_builtin_cd() {{
    local before after result
    before="$(\builtin pwd -P)" || return
    \{builtin} cd "$@"
    result=$?
    (( result == 0 )) || return "$result"
    after="$(\builtin pwd -P)" || return
    _cj_record_move "$before" "$after"
}}

\builtin unalias cd 2>/dev/null || :
function cd() {{
    local target _cj_status _cj_before _cj_after _cj_nav_arg _cj_nav_mode
    local _cj_count _cj_remaining _cj_index _cj_value
    local -a _cj_args

    if [[ $# -eq 0 ]]; then
        _cj_builtin_cd
        return
    fi

    case "${{1-}}" in
        -r|--raw)
            shift
            if [[ $# -ne 1 ]]; then
                printf '%s\n' 'cj: --raw requires exactly one target' >&2
                return 2
            fi
            _cj_builtin_cd -- "$1"
            return
            ;;
        -z|--zoxide)
            _cj_nav_arg=
            ;;
        -jw|--jump-worktree)
            if [[ $# -eq 2 && -n "$2" ]]; then
                _cj_builtin_cd -- "$2"
                return
            fi
            printf '%s\n' 'cj: type cd -jw and press Tab to select a worktree' >&2
            return 2
            ;;
        -Z|--no-zoxide)
            if [[ $# -eq 2 ]]; then
                if _cj_builtin_cd -- "$2" 2>/dev/null; then return 0; fi
                _cj_nav_arg="$2"
            fi
            ;;
        -*)
            _cj_builtin_cd "$@"
            return
            ;;
        *)
            if _cj_builtin_cd "$@" 2>/dev/null; then return 0; fi
            [[ $# -eq 1 ]] && _cj_nav_arg="$1"
            ;;
    esac

    _cj_nav_mode=other
    if _cj_is_repeated "${{_cj_nav_arg-}}" "$_cj_navigate_up"; then
        _cj_nav_mode=up
    elif _cj_is_repeated "${{_cj_nav_arg-}}" "$_cj_navigate_down"; then
        _cj_count=0
        _cj_value="$_cj_nav_arg"
        while [[ -n "$_cj_value" ]]; do
            _cj_count=$((_cj_count + 1))
            _cj_value="${{_cj_value#"$_cj_navigate_down"}}"
        done
        _cj_remaining=$((${{#_cj_history[@]}} - _cj_count))
        if (( _cj_remaining < 0 )); then
            printf '%s\n' 'cj: directory history exhausted' >&2
            return 2
        fi
        _cj_index=$_cj_remaining
        [[ -n "${{ZSH_VERSION-}}" ]] && _cj_index=$((_cj_index + 1))
        target="${{_cj_history[_cj_index]}}"
        _cj_before="$(\builtin pwd -P)" || return
        \{builtin} cd -- "$target" || return
        _cj_after="$(\builtin pwd -P)" || return
        [[ "$_cj_before" != "$_cj_after" ]] || return 0
        _cj_history=("${{_cj_history[@]:0:_cj_remaining}}")
        if typeset -f _cj_key_history_reset >/dev/null; then _cj_key_history_reset; fi
        return 0
    fi
    case "${{1-}}" in
        -z|--zoxide|-Z|--no-zoxide) _cj_args=("$@") ;;
        *) _cj_args=(-- "$@") ;;
    esac
    _cj_before="$(\builtin pwd -P)" || return
    target="$(\command cj{config} "${{_cj_args[@]}}")"
    _cj_status=$?
    (( _cj_status == 0 )) || return "$_cj_status"
    [[ -n "$target" ]] || return 1
    \{builtin} cd -- "$target" || return
    _cj_after="$(\builtin pwd -P)" || return
    _cj_record_move "$_cj_before" "$_cj_after" "$_cj_nav_mode"
}}

function _cj_complete_cd() {{
    local current candidate
    local -a destinations candidates
    destinations=({destinations})

    if [[ -n "${{BASH_VERSION-}}" ]]; then
        current="${{COMP_WORDS[COMP_CWORD]}}"
        if {{ (( COMP_CWORD == 1 )) && [[ "$current" == -jw || "$current" == --jump-worktree ]]; }} ||
            {{ (( COMP_CWORD == 2 )) && [[ "${{COMP_WORDS[1]}}" == -jw || "${{COMP_WORDS[1]}}" == --jump-worktree ]]; }}; then
            COMPREPLY=()
            while IFS= read -r -d '' candidate; do
                (( COMP_CWORD == 1 )) || [[ "$candidate" == "$current"* ]] || continue
                COMPREPLY+=("$candidate")
            done < <(\command cj{config} --worktree-paths0 2>/dev/null)
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
    if {{ (( CURRENT == 2 )) && [[ "$current" == -jw || "$current" == --jump-worktree ]]; }} ||
        {{ (( CURRENT == 3 )) && [[ "${{words[2]}}" == -jw || "${{words[2]}}" == --jump-worktree ]]; }}; then
        candidates=()
        while IFS= read -r -d '' candidate; do
            candidates+=("$candidate")
        done < <(\command cj{config} --worktree-paths0 2>/dev/null)
        (( ${{#candidates[@]}} )) || return 0
        # Replace the jump token instead of matching paths against it.
        if (( CURRENT == 2 )); then PREFIX='' SUFFIX=''; fi
        # Offer complete destinations instead of inserting their common parent.
        compstate[insert]=menu
        compadd -f -- "${{candidates[@]}}"
        return
    fi

    compadd -X 'cj destination' -- "${{destinations[@]}}"
    "${{_cj_cd_completion_fallback:-_cd}}" "$@"
}}

function _cj_register_cd_completion() {{
    (( $+functions[compdef] )) || return 0
    if [[ "${{_comps[cd]-}}" != _cj_complete_cd ]]; then
        _cj_cd_completion_fallback="${{_comps[cd]-_cd}}"
        compdef _cj_complete_cd cd
    fi
}}

if [[ -n "${{BASH_VERSION-}}" ]]; then
    complete -F _cj_complete_cd cd
elif [[ -n "${{ZSH_VERSION-}}" ]]; then
    autoload -Uz add-zsh-hook
    add-zsh-hook -d precmd _cj_register_cd_completion 2>/dev/null || :
    add-zsh-hook precmd _cj_register_cd_completion
    _cj_register_cd_completion
fi"#
    );
    let widget = crate::key_bindings::render(shell, binding, settings);
    Ok(format!("{wrapper}\n{widget}"))
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
        r#"def _cj-is-repeated [value: string, ticker: string] {{
    ($value | str length) > 0 and (($value | split chars | all {{ |char| $char == $ticker }}))
}}

def --env _cj-record-move [before: string, mode: string = 'other', logical: string = ''] {{
    let after = ($env.PWD | path expand)
    if $before == $after {{ return }}
    mut entries = [$before]
    if $mode == 'up' {{
        mut parent = ($logical | path dirname)
        mut previous = $before
        while $parent != ($parent | path dirname) {{
            let physical = ($parent | path expand)
            if $physical == $after {{ break }}
            if $physical != $previous {{ $entries = ($entries | append $physical) }}
            $previous = $physical
            $parent = ($parent | path dirname)
        }}
    }}
    $env.__cj_history = ($env.__cj_history | append $entries | last 100)
}}

def _cj-complete-cd [spans: list<string>] {{
    let current = ($spans | last)
    let attached = (($spans | length) == 2) and ($current in ['-jw' '--jump-worktree'])
    let separated = (($spans | length) == 3) and ($spans.1 in ['-jw' '--jump-worktree'])
    if $attached or $separated {{
        let prefix = if $separated {{
            try {{ $current | from nuon | into string }} catch {{
                $current | str trim --char '`' | str trim --char '"' | str trim --char "'"
            }}
        }} else {{ '' }}
        let config = {config}
        let result = (^cj ...$config --worktree --format json | complete)
        if $result.exit_code != 0 {{ return [] }}
        return (try {{
            $result.stdout | from json | where {{ |row| $row.path | str starts-with $prefix }} | each {{ |row|
                {{ value: ($row.path | to nuon), description: 'Git worktree' }}
            }}
        }} catch {{ [] }})
    }}
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
    let logical = $env.PWD
    let before = ($env.PWD | path expand)

    if ($args | is-empty) {{
        cd
        _cj-record-move $before
        return
    }}

    if ($args.0 in ['-jw' '--jump-worktree']) {{
        if (($args | length) != 2) or ($args.1 | is-empty) {{
            error make {{ msg: 'cj: type cd -jw and press Tab to select a worktree' }}
        }}
        cd $args.1
        _cj-record-move $before
        return
    }}

    if (($args.0 == '-r') or ($args.0 == '--raw')) {{
        if ($args | length) != 2 {{
            error make {{ msg: 'cj: --raw requires exactly one target' }}
        }}
        cd $args.1
        _cj-record-move $before
        return
    }}

    if (($args.0 == '-P') or ($args.0 == '--physical')) {{
        if ($args | length) > 2 {{ error make {{ msg: 'cj: --physical accepts at most one target' }} }}
        if ($args | length) == 1 {{ cd --physical }} else {{ cd --physical $args.1 }}
        _cj-record-move $before
        return
    }}

    if (($args | length) == 1) and (($args.0 == '-h') or ($args.0 == '--help')) {{
        help cd
        return
    }}

    if (($args | length) == 1) and (not ($args.0 | str starts-with '-')) {{
        let direct = (try {{ cd $args.0; true }} catch {{ false }})
        if $direct {{
            _cj-record-move $before
            return
        }}
    }} else if (($args | length) == 2) and (($args.0 == '-Z') or ($args.0 == '--no-zoxide')) {{
        let direct = (try {{ cd $args.1; true }} catch {{ false }})
        if $direct {{
            _cj-record-move $before
            return
        }}
    }} else if ($args.0 | str starts-with '-') and (not ($args.0 in ['-z' '--zoxide' '-Z' '--no-zoxide'])) {{
        if ($args | length) != 1 {{ error make {{ msg: 'cj: Nushell cd accepts one target' }} }}
        cd $args.0
        _cj-record-move $before
        return
    }}

    let nav_arg = if (($args | length) == 1) {{ $args.0 }} else if (($args | length) == 2) and (($args.0 == '-Z') or ($args.0 == '--no-zoxide')) {{ $args.1 }} else {{ '' }}
    let invoke_args = if ($args.0 in ['-z' '--zoxide' '-Z' '--no-zoxide']) {{ $args }} else {{ ['--'] | append $args }}
    if (_cj-is-repeated $nav_arg $navigate_down) {{
        let count = ($nav_arg | str length)
        let remaining = (($env.__cj_history | length) - $count)
        if $remaining < 0 {{ error make {{ msg: 'cj: directory history exhausted' }} }}
        let target = ($env.__cj_history | get $remaining)
        cd $target
        if ($env.PWD | path expand) != $before {{
            $env.__cj_history = ($env.__cj_history | first $remaining)
        }}
        return
    }}
    let result = (^cj ...$config ...$invoke_args | complete)
    if $result.exit_code != 0 {{
        if not ($result.stderr | is-empty) {{ print --stderr --no-newline $result.stderr }}
        error make {{ msg: $'cj exited with status ($result.exit_code)' }}
    }}
    let target = ($result.stdout | str replace --regex '\r?\n$' '')
    if ($target | is-empty) {{ error make {{ msg: 'cj returned an empty destination' }} }}
    cd $target
    let mode = if (_cj-is-repeated $nav_arg $navigate_up) {{ 'up' }} else {{ 'other' }}
    _cj-record-move $before $mode $logical
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
$global:__cj_history = @()
$script:__cj_navigate_up = {navigate_up}
$script:__cj_navigate_down = {navigate_down}
$script:__cj_path_comparison = if ($env:OS -eq 'Windows_NT') {{ [System.StringComparison]::OrdinalIgnoreCase }} else {{ [System.StringComparison]::Ordinal }}

function script:Test-CjRepeated {{
    param([string]$Value, [string]$Ticker)
    if ([string]::IsNullOrEmpty($Value) -or [string]::IsNullOrEmpty($Ticker)) {{ return $false }}
    return $Value.Replace($Ticker, '').Length -eq 0
}}

function script:Add-CjHistory {{
    param([string]$Before, [string]$Mode = 'other')
    $after = (Microsoft.PowerShell.Management\Get-Location).ProviderPath
    if ($after.Equals($Before, $script:__cj_path_comparison)) {{ return }}
    if (Get-Command Reset-CjKeyHistory -CommandType Function -ErrorAction SilentlyContinue) {{ Reset-CjKeyHistory }}
    $entries = @($Before)
    if ($Mode -eq 'up') {{
        $parent = [System.IO.Path]::GetDirectoryName($Before)
        while (-not [string]::IsNullOrEmpty($parent) -and -not $parent.Equals($after, $script:__cj_path_comparison)) {{
            $entries += $parent
            $parent = [System.IO.Path]::GetDirectoryName($parent)
        }}
    }}
    $global:__cj_history = @(($global:__cj_history + $entries) | Select-Object -Last 100)
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
        $json = @(& $executable @configArgs --worktree --format json 2>$null)
        if ($LASTEXITCODE -ne 0) {{ return }}
        try {{
            $worktrees = ($json -join [Environment]::NewLine) | ConvertFrom-Json -ErrorAction Stop
        }} catch {{
            return
        }}
        foreach ($worktree in @($worktrees)) {{
            $path = [string]$worktree.path
            if (-not [string]::IsNullOrEmpty($path)) {{
                $completion = ConvertTo-CjCompletionText $path
                [System.Management.Automation.CompletionResult]::new($completion, $path, 'ProviderContainer', $path)
            }}
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
    $before = (Microsoft.PowerShell.Management\Get-Location).ProviderPath
    if ($cjArgs.Count -eq 0) {{
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $HOME -ErrorAction Stop
        Add-CjHistory $before
        return
    }}
    $first = [string]$cjArgs[0]

    if (($first -ceq '-jw') -or ($first -ceq '--jump-worktree')) {{
        if ($cjArgs.Count -ne 2 -or [string]::IsNullOrEmpty([string]$cjArgs[1])) {{ throw 'cj: type cd -jw and press Tab to select a worktree' }}
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $cjArgs[1] -ErrorAction Stop
        Add-CjHistory $before
        return
    }}

    if (($first -ceq '-r') -or ($first -ceq '--raw')) {{
        if ($cjArgs.Count -ne 2) {{ throw 'cj: --raw requires exactly one target' }}
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $cjArgs[1] -ErrorAction Stop
        Add-CjHistory $before
        return
    }}

    if (($cjArgs.Count -eq 1) -and (Test-Path -LiteralPath $cjArgs[0] -PathType Container)) {{
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $cjArgs[0] -ErrorAction Stop
        Add-CjHistory $before
        return
    }}
    if (($cjArgs.Count -eq 1) -and ($first -ceq '-')) {{
        Microsoft.PowerShell.Management\Set-Location -Path '-' -ErrorAction Stop
        Add-CjHistory $before
        return
    }}
    if (($first -ceq '-Path') -and ($cjArgs.Count -eq 2)) {{
        Microsoft.PowerShell.Management\Set-Location -Path $cjArgs[1] -ErrorAction Stop
        Add-CjHistory $before
        return
    }}
    if (($first -ceq '-LiteralPath') -and ($cjArgs.Count -eq 2)) {{
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $cjArgs[1] -ErrorAction Stop
        Add-CjHistory $before
        return
    }}
    if ($first.StartsWith('-') -and -not (($first -ceq '-z') -or ($first -ceq '--zoxide') -or ($first -ceq '-Z') -or ($first -ceq '--no-zoxide'))) {{ throw "cj: unsupported PowerShell cd option: $first" }}

    $navArg = if ($cjArgs.Count -eq 1) {{ $first }} elseif (($cjArgs.Count -eq 2) -and (($first -ceq '-Z') -or ($first -ceq '--no-zoxide'))) {{ [string]$cjArgs[1] }} else {{ '' }}
    [string[]]$invokeArgs = if (($first -ceq '-z') -or ($first -ceq '--zoxide') -or ($first -ceq '-Z') -or ($first -ceq '--no-zoxide')) {{ $cjArgs }} else {{ @('--') + $cjArgs }}
    if (Test-CjRepeated $navArg $script:__cj_navigate_down) {{
        $remaining = $global:__cj_history.Count - $navArg.Length
        if ($remaining -lt 0) {{ throw 'cj: directory history exhausted' }}
        $target = $global:__cj_history[$remaining]
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $target -ErrorAction Stop
        $after = (Microsoft.PowerShell.Management\Get-Location).ProviderPath
        if (-not $after.Equals($before, $script:__cj_path_comparison)) {{
            $global:__cj_history = @($global:__cj_history | Select-Object -First $remaining)
            if (Get-Command Reset-CjKeyHistory -CommandType Function -ErrorAction SilentlyContinue) {{ Reset-CjKeyHistory }}
        }}
        return
    }}
    $configArgs = $script:__cj_config
    $executable = $script:__cj_executable.Path
    $target = @(& $executable @configArgs @invokeArgs)
    $status = $LASTEXITCODE
    if ($status -ne 0) {{ throw "cj exited with status $status" }}
    if ($target.Count -ne 1 -or [string]::IsNullOrEmpty($target[0])) {{ throw 'cj returned an invalid destination' }}
    Microsoft.PowerShell.Management\Set-Location -LiteralPath $target[0] -ErrorAction Stop
    $mode = if (Test-CjRepeated $navArg $script:__cj_navigate_up) {{ 'up' }} else {{ 'other' }}
    Add-CjHistory $before $mode
}}

if (Test-Path Function:TabExpansion2) {{
    if (-not (Test-Path Variable:script:__cj_tab_expansion2)) {{
        $script:__cj_tab_expansion2 = $function:TabExpansion2
    }}
    function global:TabExpansion2 {{
        [CmdletBinding(DefaultParameterSetName = 'ScriptInputSet')]
        param(
            [Parameter(ParameterSetName = 'ScriptInputSet', Mandatory = $true, Position = 0)]
            [string]$inputScript,
            [Parameter(ParameterSetName = 'ScriptInputSet', Position = 1)]
            [int]$cursorColumn = $inputScript.Length,
            [Parameter(ParameterSetName = 'AstInputSet', Mandatory = $true, Position = 0)]
            [System.Management.Automation.Language.Ast]$ast,
            [Parameter(ParameterSetName = 'AstInputSet', Mandatory = $true, Position = 1)]
            [System.Management.Automation.Language.Token[]]$tokens,
            [Parameter(ParameterSetName = 'AstInputSet', Mandatory = $true, Position = 2)]
            [System.Management.Automation.Language.IScriptPosition]$positionOfCursor,
            [Parameter(ParameterSetName = 'ScriptInputSet', Position = 2)]
            [Parameter(ParameterSetName = 'AstInputSet', Position = 3)]
            [hashtable]$options = $null
        )

        if ($PSCmdlet.ParameterSetName -ceq 'ScriptInputSet') {{
            $beforeCursor = $inputScript.Substring(0, $cursorColumn)
            $jump = [regex]::Match($beforeCursor, '^\s*cd\s+(?<flag>-jw|--jump-worktree)[ \t]*$')
            if ($jump.Success) {{
                $matches = @(Complete-CjCdArgument $jump.Groups['flag'].Value)
                $results = [System.Collections.ObjectModel.Collection[System.Management.Automation.CompletionResult]]::new()
                foreach ($match in $matches) {{ [void]$results.Add($match) }}
                return [System.Management.Automation.CommandCompletion]::new(
                    $results,
                    -1,
                    $jump.Groups['flag'].Index,
                    ($cursorColumn - $jump.Groups['flag'].Index)
                )
            }}
        }}
        return & $script:__cj_tab_expansion2 @PSBoundParameters
    }}
}}"#
    );
    let widget = crate::key_bindings::render(Shell::Pwsh, binding, settings);
    Ok(format!("{wrapper}\n{widget}"))
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
        assert!(output.contains("_cj_history"));
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
        assert!(bash.contains(r#"\builtin bind -x '"\C-o":_cj_key_widget'"#));
        assert!(bash.contains("--internal-key-binding-zoxide"));

        let zsh = render(Shell::Zsh, None, Some(KeyBinding::AltO), &Config::default()).unwrap();
        assert!(zsh.contains("\\builtin bindkey '^[o' _cj_key_widget"));

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
    fn wrappers_explain_that_jump_tokens_require_tab() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Nu, Shell::Pwsh] {
            let source = render(shell, None, None, &Config::default()).unwrap();
            assert!(source.contains("type cd -jw and press Tab to select a worktree"));
        }
    }

    #[test]
    fn renders_jump_worktree_completion_hooks() {
        let bash = render(Shell::Bash, None, None, &Config::default()).unwrap();
        assert!(bash.contains("(( COMP_CWORD == 1 ))"));
        assert!(bash.contains("complete -F _cj_complete_cd cd"));
        assert!(bash.contains("\\command cj --worktree-paths0"));
        assert!(bash.contains("declare -F _cd"));

        let zsh = render(Shell::Zsh, None, None, &Config::default()).unwrap();
        assert!(zsh.contains("(( CURRENT == 2 ))"));
        assert!(zsh.contains("compadd -f -- \"${candidates[@]}\""));
        assert!(zsh.contains("_cj_cd_completion_fallback"));
        assert!(zsh.contains("compdef _cj_complete_cd cd"));
        assert!(zsh.contains("add-zsh-hook precmd _cj_register_cd_completion"));

        let powershell = render(Shell::Pwsh, None, None, &Config::default()).unwrap();
        assert!(powershell.contains("function script:Complete-CjCdArgument"));
        assert!(powershell.contains("--worktree --format json"));
        assert!(powershell.contains("$cjArgs = @($args)"));
        assert!(powershell.contains("function global:TabExpansion2"));
        assert!(powershell.contains("foreach ($match in $matches)"));
        assert!(powershell.contains("$script:__cj_tab_expansion2 @PSBoundParameters"));
        assert!(powershell.contains("CompletionCompleters]::CompleteFilename"));
        assert!(!powershell.contains("Set-PSReadLineKeyHandler -Key Tab"));

        let nu = render(Shell::Nu, None, None, &Config::default()).unwrap();
        assert!(nu.contains("--worktree --format json"));
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
