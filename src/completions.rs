use std::collections::BTreeSet;

use crate::cli::Shell;
use crate::config::Config;

const FLAGS: &[&str] = &[
    "-C",
    "--config",
    "-v",
    "--verbose",
    "-r",
    "--raw",
    "-z",
    "--zoxide",
    "-Z",
    "--no-zoxide",
    "-w",
    "--worktree",
    "-f",
    "--format",
    "-R",
    "--relative",
    "--pick-worktree",
    "-h",
    "--help",
    "-V",
    "--version",
];
const COMMON_FLAGS: &[&str] = &[
    "-C",
    "--config",
    "-v",
    "--verbose",
    "-h",
    "--help",
    "-V",
    "--version",
];
const COMMANDS: &[&str] = &["completions", "config", "init", "mounts"];
const SHELLS: &[&str] = &["bash", "zsh", "nu", "powershell"];
const FORMATS: &[&str] = &["table", "json"];

pub fn render(shell: Shell, config: &Config) -> String {
    let destinations = destinations(config);
    match shell {
        Shell::Bash => bash(&destinations),
        Shell::Zsh => zsh(&destinations),
        Shell::Nu => nu(&destinations),
        Shell::Pwsh => powershell(&destinations),
    }
}

pub(crate) fn destinations(config: &Config) -> Vec<String> {
    config
        .keywords
        .top
        .iter()
        .chain(&config.keywords.main_worktree)
        .chain(config.aliases.keys())
        .chain(config.mounts.keys())
        .cloned()
        .chain([
            config.tickers.navigate_up.clone(),
            config.tickers.navigate_down.clone(),
        ])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn bash(destinations: &[String]) -> String {
    let destinations = shell_array(destinations, quote_posix);
    let flags = shell_array(FLAGS, quote_posix);
    let common_flags = shell_array(COMMON_FLAGS, quote_posix);
    let commands = shell_array(COMMANDS, quote_posix);
    let shells = shell_array(SHELLS, quote_posix);
    let formats = shell_array(FORMATS, quote_posix);
    format!(
        r#"_cj_complete() {{
    local current previous command subcommand token candidate i command_value forced_resolve worktree_mode pick_mode literal_mode
    local -a candidates destinations flags common_flags commands shells formats
    COMPREPLY=()
    current="${{COMP_WORDS[COMP_CWORD]}}"
    previous="${{COMP_WORDS[COMP_CWORD-1]-}}"
    destinations=({destinations})
    flags=({flags})
    common_flags=({common_flags})
    commands=({commands})
    shells=({shells})
    formats=({formats})

    literal_mode=
    for ((i = 1; i < COMP_CWORD; i++)); do
        [[ "${{COMP_WORDS[i]}}" == -- ]] && literal_mode=1 && break
    done
    if [[ -n "$literal_mode" ]]; then
        candidates=("${{destinations[@]}}")
    else case "$previous" in
        -C|--config|-o|--output)
            compopt -o filenames 2>/dev/null || :
            while IFS= read -r candidate; do COMPREPLY+=("$candidate"); done < <(compgen -f -- "$current")
            return
            ;;
        -f|--format) candidates=("${{formats[@]}}") ;;
        *)
            command=
            subcommand=
            command_value=
            forced_resolve=
            worktree_mode=
            pick_mode=
            i=1
            while (( i < COMP_CWORD )); do
                token="${{COMP_WORDS[i]}}"
                case "$token" in
                    -C|--config|-f|--format|-o|--output) ((i += 2)); continue ;;
                    --) forced_resolve=1; command=; subcommand=; break ;;
                    -r|--raw|-z|--zoxide|-Z|--no-zoxide) forced_resolve=1; command=; subcommand=; ((i++)); continue ;;
                    -w|--worktree) worktree_mode=1; ((i++)); continue ;;
                    --pick-worktree) worktree_mode=1; pick_mode=1; ((i++)); continue ;;
                    config|mounts|init|completions)
                        [[ -n "$forced_resolve" ]] && break
                        if [[ -z "$command" ]]; then command="$token"; else subcommand="$token"; fi
                        ((i++)); continue
                        ;;
                    scan)
                        [[ "$command" == mounts ]] && subcommand=scan
                        ((i++)); continue
                        ;;
                    -*) ((i++)); continue ;;
                    *)
                        [[ "$command" == init || "$command" == completions ]] && command_value=1
                        break
                        ;;
                esac
            done
            if [[ -n "$forced_resolve" ]]; then
                candidates=("${{destinations[@]}}" "${{common_flags[@]}}")
            elif [[ -n "$pick_mode" ]]; then
                candidates=("${{common_flags[@]}}")
            elif [[ -n "$worktree_mode" ]]; then
                candidates=(-f --format -R --relative "${{common_flags[@]}}")
            else case "$command:$subcommand" in
                config:) candidates=(init "${{common_flags[@]}}") ;;
                config:init) candidates=(--preamp "${{common_flags[@]}}") ;;
                mounts:) candidates=(scan "${{common_flags[@]}}") ;;
                mounts:scan) candidates=(-f --format "${{common_flags[@]}}") ;;
                init:) if [[ -n "$command_value" ]]; then candidates=(-o --output --setup-key-binding "${{common_flags[@]}}"); else candidates=("${{shells[@]}}" -o --output --setup-key-binding "${{common_flags[@]}}"); fi ;;
                completions:) if [[ -n "$command_value" ]]; then candidates=("${{common_flags[@]}}"); else candidates=("${{shells[@]}}" "${{common_flags[@]}}"); fi ;;
                *) candidates=("${{commands[@]}}" "${{destinations[@]}}" "${{flags[@]}}") ;;
            esac; fi
            ;;
    esac; fi

    for candidate in "${{candidates[@]}}"; do
        [[ "$candidate" == "$current"* ]] && COMPREPLY+=("$candidate")
    done
}}
complete -F _cj_complete cj"#
    )
}

fn zsh(destinations: &[String]) -> String {
    let destinations = shell_array(destinations, quote_posix);
    let flags = shell_array(FLAGS, quote_posix);
    let common_flags = shell_array(COMMON_FLAGS, quote_posix);
    let commands = shell_array(COMMANDS, quote_posix);
    let shells = shell_array(SHELLS, quote_posix);
    let formats = shell_array(FORMATS, quote_posix);
    format!(
        r#"#compdef cj
_cj_complete() {{
    local previous command subcommand token command_value forced_resolve worktree_mode pick_mode literal_mode
    local -i i
    local -a candidates destinations flags common_flags commands shells formats
    previous="${{words[CURRENT-1]-}}"
    destinations=({destinations})
    flags=({flags})
    common_flags=({common_flags})
    commands=({commands})
    shells=({shells})
    formats=({formats})

    literal_mode=
    for ((i = 2; i < CURRENT; i++)); do
        [[ "${{words[i]}}" == -- ]] && literal_mode=1 && break
    done
    if [[ -n "$literal_mode" ]]; then
        candidates=("${{destinations[@]}}")
    else case "$previous" in
        -C|--config|-o|--output) _files; return ;;
        -f|--format) candidates=("${{formats[@]}}") ;;
        *)
            command=
            subcommand=
            command_value=
            forced_resolve=
            worktree_mode=
            pick_mode=
            i=2
            while (( i < CURRENT )); do
                token="${{words[i]}}"
                case "$token" in
                    -C|--config|-f|--format|-o|--output) ((i += 2)); continue ;;
                    --) forced_resolve=1; command=; subcommand=; break ;;
                    -r|--raw|-z|--zoxide|-Z|--no-zoxide) forced_resolve=1; command=; subcommand=; ((i++)); continue ;;
                    -w|--worktree) worktree_mode=1; ((i++)); continue ;;
                    --pick-worktree) worktree_mode=1; pick_mode=1; ((i++)); continue ;;
                    config|mounts|init|completions)
                        [[ -n "$forced_resolve" ]] && break
                        if [[ -z "$command" ]]; then command="$token"; else subcommand="$token"; fi
                        ((i++)); continue
                        ;;
                    scan)
                        [[ "$command" == mounts ]] && subcommand=scan
                        ((i++)); continue
                        ;;
                    -*) ((i++)); continue ;;
                    *)
                        [[ "$command" == init || "$command" == completions ]] && command_value=1
                        break
                        ;;
                esac
            done
            if [[ -n "$forced_resolve" ]]; then
                candidates=("${{destinations[@]}}" "${{common_flags[@]}}")
            elif [[ -n "$pick_mode" ]]; then
                candidates=("${{common_flags[@]}}")
            elif [[ -n "$worktree_mode" ]]; then
                candidates=(-f --format -R --relative "${{common_flags[@]}}")
            else case "$command:$subcommand" in
                config:) candidates=(init "${{common_flags[@]}}") ;;
                config:init) candidates=(--preamp "${{common_flags[@]}}") ;;
                mounts:) candidates=(scan "${{common_flags[@]}}") ;;
                mounts:scan) candidates=(-f --format "${{common_flags[@]}}") ;;
                init:) if [[ -n "$command_value" ]]; then candidates=(-o --output --setup-key-binding "${{common_flags[@]}}"); else candidates=("${{shells[@]}}" -o --output --setup-key-binding "${{common_flags[@]}}"); fi ;;
                completions:) if [[ -n "$command_value" ]]; then candidates=("${{common_flags[@]}}"); else candidates=("${{shells[@]}}" "${{common_flags[@]}}"); fi ;;
                *) candidates=("${{commands[@]}}" "${{destinations[@]}}" "${{flags[@]}}") ;;
            esac; fi
            ;;
    esac; fi
    compadd -- "${{candidates[@]}}"
}}
compdef _cj_complete cj"#
    )
}

fn nu(destinations: &[String]) -> String {
    let destinations = destinations
        .iter()
        .cloned()
        .chain(COMMANDS.iter().map(|value| (*value).to_owned()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let destinations = nu_records(&destinations);
    format!(
        r#"def "nu-complete cj destinations" [] {{
    [{destinations}]
}}

def "nu-complete cj shells" [] {{
    [bash zsh nu powershell]
}}

def "nu-complete cj formats" [] {{
    [table json]
}}

export extern cj [
    ...target: string@"nu-complete cj destinations"
    --config(-C): path
    --verbose(-v)
    --raw(-r)
    --zoxide(-z)
    --no-zoxide(-Z)
    --worktree(-w)
    --format(-f): string@"nu-complete cj formats"
    --relative(-R)
    --pick-worktree
    --help(-h)
    --version(-V)
]

export extern "cj init" [
    shell: string@"nu-complete cj shells"
    --setup-key-binding
    --output(-o): path
    --config(-C): path
    --verbose(-v)
    --help(-h)
    --version(-V)
]

export extern "cj completions" [
    shell: string@"nu-complete cj shells"
    --config(-C): path
    --verbose(-v)
    --help(-h)
    --version(-V)
]

export extern "cj config init" [
    --preamp
    --config(-C): path
    --verbose(-v)
    --help(-h)
    --version(-V)
]

export extern "cj config" [
    --config(-C): path
    --verbose(-v)
    --help(-h)
    --version(-V)
]

export extern "cj mounts" [
    --config(-C): path
    --verbose(-v)
    --help(-h)
    --version(-V)
]

export extern "cj mounts scan" [
    --format(-f): string@"nu-complete cj formats"
    --config(-C): path
    --verbose(-v)
    --help(-h)
    --version(-V)
]"#
    )
}

fn powershell(destinations: &[String]) -> String {
    let destinations = ps_array(destinations);
    let flags = ps_array(FLAGS);
    let common_flags = ps_array(COMMON_FLAGS);
    let commands = ps_array(COMMANDS);
    let shells = ps_array(SHELLS);
    let formats = ps_array(FORMATS);
    format!(
        r#"Register-ArgumentCompleter -Native -CommandName @('cj', 'cj.exe') -ScriptBlock {{
    param($wordToComplete, $commandAst, $cursorPosition)
    $destinations = @({destinations})
    $flags = @({flags})
    $commonFlags = @({common_flags})
    $commands = @({commands})
    $shells = @({shells})
    $formats = @({formats})
    $elements = @($commandAst.CommandElements | Where-Object {{ $_.Extent.StartOffset -lt $cursorPosition }} | ForEach-Object {{ $_.Extent.Text }})
    $previous = if ([string]::IsNullOrEmpty($wordToComplete)) {{ if ($elements.Count -gt 0) {{ $elements[-1] }} else {{ '' }} }} elseif ($elements.Count -gt 1) {{ $elements[-2] }} else {{ '' }}
    $literalMode = $elements -ccontains '--'

    if ($literalMode) {{
        $candidates = $destinations
    }} elseif (@('-C', '--config', '-o', '--output') -ccontains $previous) {{
        return
    }} elseif (@('-f', '--format') -ccontains $previous) {{
        $candidates = $formats
    }} else {{
        $command = ''
        $subcommand = ''
        $commandValue = $false
        $forcedResolve = $false
        $worktreeMode = $false
        $pickMode = $false
        $skipValue = $false
        $scanLimit = $elements.Count
        if (-not [string]::IsNullOrEmpty($wordToComplete)) {{ $scanLimit-- }}
        for ($index = 1; $index -lt $scanLimit; $index++) {{
            $token = $elements[$index]
            if ($skipValue) {{ $skipValue = $false; continue }}
            if (($token -ceq '-C') -or ($token -ceq '--config') -or ($token -ceq '-f') -or ($token -ceq '--format') -or ($token -ceq '-o') -or ($token -ceq '--output')) {{ $skipValue = $true; continue }}
            if ($token -ceq '--') {{ $forcedResolve = $true; $command = ''; $subcommand = ''; break }}
            if (($token -ceq '-r') -or ($token -ceq '--raw') -or ($token -ceq '-z') -or ($token -ceq '--zoxide') -or ($token -ceq '-Z') -or ($token -ceq '--no-zoxide')) {{ $forcedResolve = $true; $command = ''; $subcommand = ''; continue }}
            if (($token -ceq '-w') -or ($token -ceq '--worktree')) {{ $worktreeMode = $true; continue }}
            if ($token -ceq '--pick-worktree') {{ $worktreeMode = $true; $pickMode = $true; continue }}
            if ($token.StartsWith('-')) {{ continue }}
            if ([string]::IsNullOrEmpty($command)) {{
                if (-not $forcedResolve -and ($commands -ccontains $token)) {{ $command = $token; continue }}
                break
            }}
            if (($command -ceq 'config') -and ($token -ceq 'init')) {{ $subcommand = 'init'; continue }}
            if (($command -ceq 'mounts') -and ($token -ceq 'scan')) {{ $subcommand = 'scan'; continue }}
            if (($command -ceq 'init') -or ($command -ceq 'completions')) {{ $commandValue = $true }}
            break
        }}
        $candidates = if ($forcedResolve) {{ $destinations + $commonFlags }} elseif ($pickMode) {{ $commonFlags }} elseif ($worktreeMode) {{ @('-f', '--format', '-R', '--relative') + $commonFlags }} else {{ switch ("$command`:$subcommand") {{
            'config:' {{ @('init') + $commonFlags }}
            'config:init' {{ @('--preamp') + $commonFlags }}
            'mounts:' {{ @('scan') + $commonFlags }}
            'mounts:scan' {{ @('-f', '--format') + $commonFlags }}
            'init:' {{ if ($commandValue) {{ @('-o', '--output', '--setup-key-binding') + $commonFlags }} else {{ $shells + @('-o', '--output', '--setup-key-binding') + $commonFlags }} }}
            'completions:' {{ if ($commandValue) {{ $commonFlags }} else {{ $shells + $commonFlags }} }}
            default {{ $commands + $destinations + $flags }}
        }} }}
    }}

    $candidates | Where-Object {{ $_.StartsWith($wordToComplete, [System.StringComparison]::OrdinalIgnoreCase) }} | ForEach-Object {{
        $completionText = if ($_ -match '[^\p{{L}}\p{{N}}_./:\\-]') {{ "'" + $_.Replace("'", "''") + "'" }} else {{ $_ }}
        [System.Management.Automation.CompletionResult]::new($completionText, $_, 'ParameterValue', $_)
    }}
}}"#
    )
}

fn shell_array<T: AsRef<str>>(values: &[T], quote: fn(&str) -> String) -> String {
    values
        .iter()
        .map(|value| quote(value.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ps_array<T: AsRef<str>>(values: &[T]) -> String {
    values
        .iter()
        .map(|value| quote_ps(value.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn nu_records(values: &[String]) -> String {
    values
        .iter()
        .map(|value| {
            format!(
                "{{ value: {}, description: 'configured destination' }}",
                quote_nu(value)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn quote_posix(value: &str) -> String {
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

fn quote_ps(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn configured() -> Config {
        let mut config = Config::default();
        config
            .aliases
            .insert("work space".into(), PathBuf::from("/tmp/work"));
        config
            .aliases
            .insert("it's-here".into(), PathBuf::from("/tmp/quote"));
        config
    }

    #[test]
    fn destinations_are_sorted_and_deduplicated() {
        let values = destinations(&configured());
        assert_eq!(values.iter().filter(|value| *value == "top").count(), 1);
        assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn generated_completions_quote_configured_names() {
        let config = configured();
        let bash = render(Shell::Bash, &config);
        assert!(bash.contains("'work space'"));
        assert!(bash.contains("'it'\\''s-here'"));

        let nu = render(Shell::Nu, &config);
        assert!(nu.contains("r#'work space'#"));
        assert!(nu.contains("r#'it's-here'#"));

        let powershell = render(Shell::Pwsh, &config);
        assert!(powershell.contains("'work space'"));
        assert!(powershell.contains("'it''s-here'"));
    }

    #[test]
    fn generated_completions_offer_init_output() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Nu, Shell::Pwsh] {
            let output = render(shell, &configured());
            assert!(output.contains("-o"));
            assert!(output.contains("--output"));
        }
    }

    #[test]
    fn nushell_raw_literals_expand_their_delimiter() {
        assert_eq!(quote_nu("plain's value"), "r#'plain's value'#");
        assert_eq!(quote_nu("closing'#marker"), "r##'closing'#marker'##");
    }
}
