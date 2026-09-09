use crate::cli::Shell;

/// Shell-local, read-only completion of the browser history. Cycling restores
/// both the original line and cursor after the oldest remembered directory.
#[allow(dead_code)] // Wired into the ordered binding dispatcher separately.
pub fn render(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash | Shell::Zsh => POSIX,
        Shell::Pwsh => POWERSHELL,
        Shell::Nu => "",
    }
}

const POSIX: &str = r#"function _cj_key_history_reset() {
    unset _cj_key_history_original _cj_key_history_original_cursor
    unset _cj_key_history_last _cj_key_history_last_cursor _cj_key_history_next
    unset _cj_key_history_items
}
_cj_key_history_reset

function _cj_key_history() {
    local buffer="$1" cursor="$2" target quoted index remainder
    # Bash 5+ and ZLE use character offsets; older Readline uses bytes.
    if [[ -n "${BASH_VERSION-}" ]] && (( BASH_VERSINFO[0] < 5 )); then local LC_ALL=C; fi
    _cj_key_buffer="$buffer"
    _cj_key_cursor="$cursor"
    if [[ -z "${_cj_key_history_next-}" || "$buffer" != "${_cj_key_history_last-}" || "$cursor" != "${_cj_key_history_last_cursor-}" ]]; then
        _cj_key_history_reset
        (( ${#_cj_history[@]} > 0 )) || return 1
        _cj_key_history_original="$buffer"
        _cj_key_history_original_cursor="$cursor"
        _cj_key_history_items=("${_cj_history[@]}")
        _cj_key_history_next=${#_cj_key_history_items[@]}
    fi
    if (( _cj_key_history_next == 0 )); then
        _cj_key_buffer="$_cj_key_history_original"
        _cj_key_cursor="$_cj_key_history_original_cursor"
        _cj_key_history_reset
        return 0
    fi
    index=$((_cj_key_history_next - 1))
    [[ -n "${ZSH_VERSION-}" ]] && index=$((index + 1))
    target="${_cj_key_history_items[$index]}"
    _cj_key_history_next=$((_cj_key_history_next - 1))
    # Single quotes preserve spaces, newlines, and metacharacters literally.
    quoted="'"
    remainder="$target"
    while [[ "$remainder" == *"'"* ]]; do
        quoted="$quoted${remainder%%\'*}'\\''"
        remainder="${remainder#*\'}"
    done
    quoted="$quoted$remainder'"
    _cj_key_buffer="$_cj_key_history_original"
    if [[ -n "$_cj_key_buffer" && "$_cj_key_buffer" != *[[:space:]] ]]; then
        _cj_key_buffer="$_cj_key_buffer "
    fi
    _cj_key_buffer="$_cj_key_buffer$quoted"
    _cj_key_cursor=${#_cj_key_buffer}
    _cj_key_history_last="$_cj_key_buffer"
    _cj_key_history_last_cursor="$_cj_key_cursor"
    return 0
}"#;

const POWERSHELL: &str = r#"function global:Reset-CjKeyHistory {
    $global:__cj_key_history_cycle = $null
}
Reset-CjKeyHistory

function global:Invoke-CjKeyHistory {
    param([string] $Buffer, [int] $Cursor)
    $cycle = $global:__cj_key_history_cycle
    if ($null -eq $cycle -or $Buffer -cne $cycle.LastBuffer -or $Cursor -ne $cycle.LastCursor) {
        Reset-CjKeyHistory
        $history = Get-Variable -Name __cj_history -Scope Global -ErrorAction SilentlyContinue
        $items = @()
        if ($null -ne $history) { $items = @($history.Value) }
        if ($items.Count -eq 0) { return $null }
        $cycle = @{
            OriginalBuffer = $Buffer; OriginalCursor = $Cursor
            LastBuffer = $Buffer; LastCursor = $Cursor
            Items = $items; Next = $items.Count - 1
        }
        $global:__cj_key_history_cycle = $cycle
    }
    if ($cycle.Next -lt 0) {
        $result = @{ Buffer = $cycle.OriginalBuffer; Cursor = $cycle.OriginalCursor }
        Reset-CjKeyHistory
        return $result
    }
    $target = [string] $cycle.Items[$cycle.Next]
    $cycle.Next -= 1
    $completed = $cycle.OriginalBuffer
    if ($completed.Length -gt 0 -and -not [char]::IsWhiteSpace($completed[$completed.Length - 1])) {
        $completed += ' '
    }
    $completed += "'" + [System.Management.Automation.Language.CodeGeneration]::EscapeSingleQuotedStringContent($target) + "'"
    $cycle.LastBuffer = $completed
    $cycle.LastCursor = $completed.Length
    return @{ Buffer = $completed; Cursor = $completed.Length }
}"#;

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::render;
    use crate::cli::Shell;

    fn run(shell: Shell, script: &str) {
        let (program, args): (&str, &[&str]) = match shell {
            Shell::Bash => ("bash", &["--noprofile", "--norc", "-c"]),
            Shell::Zsh => ("zsh", &["-f", "-c"]),
            Shell::Pwsh => (
                "pwsh",
                &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"],
            ),
            Shell::Nu => unreachable!(),
        };
        let source = format!("{}\n{script}", render(shell));
        let output = match Command::new(program).args(args).arg(source).output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                assert!(
                    std::env::var_os("CJ_REQUIRE_SHELLS").is_none()
                        && !(matches!(shell, Shell::Pwsh)
                            && std::env::var_os("CJ_REQUIRE_POWERSHELL").is_some()),
                    "required interpreter unavailable: {program}"
                );
                eprintln!("skipping {program} history binding test: interpreter unavailable");
                return;
            }
            Err(error) => panic!("cannot run {program}: {error}"),
        };
        assert!(
            output.status.success(),
            "{program}: status {}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Windows may expose a bash.exe WSL launcher without an installed distribution.
    #[cfg(unix)]
    #[test]
    fn posix_history_completion_is_reversible_and_read_only() {
        for shell in [Shell::Bash, Shell::Zsh] {
            run(
                shell,
                r#"set -e
_cj_before=$PWD
_cj_down_route='/remembered/down/route'
_cj_history=()
if _cj_key_history 'cd ' 1; then exit 10; fi
[[ $_cj_key_buffer == 'cd ' && $_cj_key_cursor == 1 ]]

_cj_history=('/single')
_cj_key_history 'cd ' 1
[[ $_cj_key_buffer == "cd '/single'" ]]
_cj_key_history "$_cj_key_buffer" "$_cj_key_cursor"
[[ $_cj_key_buffer == 'cd ' && $_cj_key_cursor == 1 ]]

_cj_history=('/oldest' '/middle' '/newest')
_cj_key_history 'cd' 0
[[ $_cj_key_buffer == "cd '/newest'" ]]
_cj_key_history "$_cj_key_buffer" "$_cj_key_cursor"
[[ $_cj_key_buffer == "cd '/middle'" ]]
_cj_key_history "$_cj_key_buffer" "$_cj_key_cursor"
[[ $_cj_key_buffer == "cd '/oldest'" ]]
_cj_key_history "$_cj_key_buffer" "$_cj_key_cursor"
[[ $_cj_key_buffer == cd && $_cj_key_cursor == 0 ]]
[[ ${#_cj_history[@]} == 3 ]]
[[ $PWD == "$_cj_before" && $_cj_down_route == /remembered/down/route ]]
"#,
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn posix_edit_cursor_change_cancel_and_reinit_restart_history_cycle() {
        for shell in [Shell::Bash, Shell::Zsh] {
            run(
                shell,
                r#"set -e
_cj_history=('/old' '/new')
_cj_key_history 'cd ' 3
_cj_key_history 'ls ' 3
[[ $_cj_key_buffer == "ls '/new'" ]]
_cj_key_history "$_cj_key_buffer" "$_cj_key_cursor"
[[ $_cj_key_buffer == "ls '/old'" ]]
_cj_key_history "$_cj_key_buffer" "$_cj_key_cursor"
[[ $_cj_key_buffer == 'ls ' && $_cj_key_cursor == 3 ]]

_cj_key_history 'cd ' 3
_cj_key_history "$_cj_key_buffer" 0
[[ $_cj_key_buffer == "cd '/new' '/new'" ]]
_cj_key_history_reset
_cj_key_history 'cd ' 3
[[ $_cj_key_buffer == "cd '/new'" ]]
# Reinitialization calls this same reset without changing browser history.
_cj_key_history_reset
[[ ${#_cj_history[@]} == 2 ]]
_cj_key_history 'cd ' 3
[[ $_cj_key_buffer == "cd '/new'" ]]
"#,
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn posix_history_quotes_special_paths_and_preserves_multiline_buffer() {
        for shell in [Shell::Bash, Shell::Zsh] {
            run(
                shell,
                r#"set -e
for target in '/space dir' "/apostrophe's" '/literal;$()`"*?[x]\' $'/line\nbreak' '/unicode/日本語‘’'; do
    _cj_key_history_reset
    _cj_history=("$target")
    _cj_key_history 'set --' 6
    # Evaluation is only in this test, modeling normal Enter execution.
    eval "$_cj_key_buffer"
    [[ $# == 1 && $1 == "$target" ]]
done
original=$'printf first\ncd '
_cj_key_history_reset
_cj_key_history "$original" 2
[[ $_cj_key_buffer == "$original"* ]]
_cj_key_history "$_cj_key_buffer" "$_cj_key_cursor"
[[ $_cj_key_buffer == "$original" && $_cj_key_cursor == 2 ]]
"#,
            );
        }
    }

    #[test]
    fn powershell_history_cycles_resets_and_quotes_literals() {
        run(
            Shell::Pwsh,
            r#"$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$before = (Get-Location).Path
$global:__cj_down_route = '/down/route'
$global:__cj_history = @()
if ($null -ne (Invoke-CjKeyHistory -Buffer 'cd ' -Cursor 1)) { throw 'empty history completed' }
$global:__cj_history = @('/single')
$r = Invoke-CjKeyHistory -Buffer 'cd ' -Cursor 1
if ($r.Buffer -cne "cd '/single'") { throw 'single history not completed' }
$r = Invoke-CjKeyHistory -Buffer $r.Buffer -Cursor $r.Cursor
if ($r.Buffer -cne 'cd ' -or $r.Cursor -ne 1) { throw 'original cursor not restored' }
$global:__cj_history = @('/old', '/middle', '/new')
$r = Invoke-CjKeyHistory -Buffer 'cd' -Cursor 0
foreach ($expected in @('/new', '/middle', '/old')) {
    if ($r.Buffer -cne "cd '$expected'") { throw "wrong cycle: $($r.Buffer)" }
    $r = Invoke-CjKeyHistory -Buffer $r.Buffer -Cursor $r.Cursor
}
if ($r.Buffer -cne 'cd' -or $r.Cursor -ne 0) { throw 'original not restored' }
$r = Invoke-CjKeyHistory -Buffer 'cd ' -Cursor 3
$r = Invoke-CjKeyHistory -Buffer 'ls ' -Cursor 3
if ($r.Buffer -cne "ls '/new'") { throw 'editing failed to restart' }
Reset-CjKeyHistory
$r = Invoke-CjKeyHistory -Buffer 'cd ' -Cursor 3
if ($r.Buffer -cne "cd '/new'") { throw 'cancellation failed to restart' }
if ($global:__cj_history.Count -ne 3 -or (Get-Location).Path -cne $before -or $global:__cj_down_route -cne '/down/route') { throw 'completion mutated navigation state' }
foreach ($target in @('/space dir', "/apostrophe's", '/literal;$()`"*?[x]\', "/line`nbreak", "/unicode/日本語$([char]0x2018)$([char]0x2019)$([char]0x201A)$([char]0x201B)")) {
    Reset-CjKeyHistory
    $global:__cj_history = @($target)
    $r = Invoke-CjKeyHistory -Buffer '' -Cursor 0
    $literal = & ([scriptblock]::Create($r.Buffer))
    if ($literal -cne $target) { throw "path did not round-trip: $target" }
}
"#,
        );
    }
}
