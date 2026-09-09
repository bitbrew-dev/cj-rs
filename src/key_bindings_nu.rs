//! Nushell's host-command event runs the picker in the foreground while keeping
//! the pending command in Reedline, without executing or recording that command.
use crate::config::{Config, KeyBinding, KeyBindingBehavior};

pub fn cleanup() -> &'static str {
    r#"export-env {
    $env.__cj_history = []
    $env.__cj_key_cycle = null
    $env.__cj_key_resume = false
    $env.config.keybindings = ($env.config.keybindings | where name != 'cj-directory')
}"#
}

pub fn render(chord: KeyBinding, config: &Config) -> String {
    let behaviors = config
        .key_binding_behaviors()
        .iter()
        .map(|behavior| match behavior {
            KeyBindingBehavior::Zoxide => "'zoxide'",
            KeyBindingBehavior::Cj => "'cj'",
        })
        .collect::<Vec<_>>()
        .join(" ");
    let zoxide = quote(&config.programs.zoxide.to_string_lossy());
    let fzf = quote(&config.programs.fzf.to_string_lossy());
    // Substitute only template tokens, never tokens inside configured paths.
    SOURCE
        .split('@')
        .map(|part| match part {
            "BEHAVIORS" => behaviors.as_str(),
            "ZOXIDE" => zoxide.as_str(),
            "FZF" => fzf.as_str(),
            "MODIFIER" if chord == KeyBinding::CtrlO => "control",
            "MODIFIER" => "alt",
            text => text,
        })
        .collect()
}

fn quote(value: &str) -> String {
    let mut hashes = "#".to_owned();
    while value.contains(&format!("'{hashes}")) {
        hashes.push('#');
    }
    format!("r{hashes}'{value}'{hashes}")
}

const SOURCE: &str = r#"def _cj-key-quote [value: string] {
    mut hashes = '#'
    while ($value | str contains ("'" + $hashes)) { $hashes += '#' }
    'r' + $hashes + "'" + $value + "'" + $hashes
}

def _cj-key-append [buffer: string, target: string] {
    let path = if ($target | str starts-with '-') { './' + $target } else { $target }
    let separator = if ($buffer | is-empty) or ($buffer =~ '\s$') { '' } else { ' ' }
    $buffer + $separator + (_cj-key-quote $path)
}

def --env _cj-key-history [buffer: string, cursor: int] {
    mut cycle = ($env.__cj_key_cycle? | default null)
    if ($cycle == null) or ($buffer != $cycle.last_buffer) or ($cursor != $cycle.last_cursor) {
        $env.__cj_key_cycle = null
        let items = ($env.__cj_history? | default [])
        if ($items | is-empty) { return null }
        $cycle = {original_buffer: $buffer, original_cursor: $cursor,
            last_buffer: $buffer, last_cursor: $cursor,
            items: $items, next: (($items | length) - 1)}
    }
    if $cycle.next < 0 {
        $env.__cj_key_cycle = null
        return {buffer: $cycle.original_buffer, cursor: $cycle.original_cursor}
    }
    let completed = (_cj-key-append $cycle.original_buffer ($cycle.items | get $cycle.next))
    let position = ($completed | str length --grapheme-clusters)
    $cycle.next = $cycle.next - 1
    $cycle.last_buffer = $completed
    $cycle.last_cursor = $position
    $env.__cj_key_cycle = $cycle
    {buffer: $completed, cursor: $position}
}

export def --env _cj-key-widget [] {
    let buffer = (commandline)
    let cursor = (commandline get-cursor)
    # Host commands run pre_prompt too. Retain this cycle for that prompt only.
    $env.__cj_key_resume = true
    let behaviors = [@BEHAVIORS@]
    let cycle = ($env.__cj_key_cycle? | default null)
    if ($cycle != null) and ($buffer == $cycle.last_buffer) and ($cursor == $cycle.last_cursor) {
        let result = (_cj-key-history $buffer $cursor)
        commandline edit --replace $result.buffer
        commandline set-cursor $result.cursor
        return
    }
    $env.__cj_key_cycle = null
    mut missing = ''
    mut empty = 'no matching directories'
    for behavior in $behaviors {
        if $behavior == 'cj' {
            let result = (_cj-key-history $buffer $cursor)
            if $result != null {
                commandline edit --replace $result.buffer
                commandline set-cursor $result.cursor
                return
            }
            $empty = 'no directory history'
            continue
        }
        # Decode explicitly: implicit external string capture trims a final LF.
        # Keep stderr attached to the terminal while the picker is foreground.
        let reply = (try {
            ^cj --internal-key-binding-zoxide @ZOXIDE@ @FZF@ | into binary | decode utf-8
        } catch { null })
        if $reply == null { return }
        if ($reply | str starts-with "selected\n") {
            let target = ($reply | str substring 9..)
            let completed = (_cj-key-append $buffer $target)
            commandline edit --replace $completed
            commandline set-cursor --end
            return
        }
        if ($reply | str starts-with "unavailable\n") {
            $missing = ($reply | str substring 12..)
        } else if $reply == "cancelled\n" {
            return
        } else if $reply != "empty\n" {
            print --stderr 'cj: invalid interactive zoxide response'
            return
        }
    }
    print --stderr ('cj: ' + (if ($missing | is-empty) { $empty } else { $missing }))
}

export-env {
    $env.__cj_history = []
    $env.__cj_key_cycle = null
    $env.__cj_key_resume = false
    if not ($env.__cj_key_prompt_hook? | default false) {
        $env.config.hooks.pre_prompt ++= [{||
            if ($env.__cj_key_resume? | default false) {
                $env.__cj_key_resume = false
            } else {
                $env.__cj_key_cycle = null
            }
        }]
        $env.__cj_key_prompt_hook = true
    }
    $env.config.keybindings = ($env.config.keybindings | where name != 'cj-directory')
    $env.config.keybindings ++= [{name: cj-directory, modifier: @MODIFIER@,
        keycode: char_o, mode: [emacs vi_insert vi_normal],
        event: {send: executehostcommand, cmd: '_cj-key-widget'}}]
}"#;
