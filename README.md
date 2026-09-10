# cj

`cj` is a small shell companion for jumping between useful directories. It adds
named aliases and mounts, Git-aware shortcuts, optional zoxide resolution, and an
fzf worktree jump while leaving the final directory change to your shell's real
`cd` builtin.

## Install

### Homebrew

The [Homebrew formula](https://github.com/benbenbang/homebrew-forge/blob/main/Formula/cj.rb)
installs a prebuilt binary on macOS or Linux for ARM64 or x86_64 (Intel/AMD).
The release repository is private, so provide a GitHub token that can read it:

```bash
export HOMEBREW_GITHUB_TEAM_BITBREW_DEV_API_TOKEN="$(gh auth token)"
brew install benbenbang/forge/cj
```

### Cargo

```console
cargo install cj-rs
```

Building from source requires Rust 1.96.0 or newer. The same minimum version is
pinned in `rust-toolchain.toml` and exercised by CI.

### Windows

Download `cj-rs-<version>-x86_64-pc-windows-msvc.zip` from
[GitHub Releases](https://github.com/bitbrew-dev/cj-rs/releases), extract it,
and put `cj.exe` on `PATH`.

`cj` supports Bash, Zsh, Nushell, and PowerShell. Generate integration and
completion files, then load them from the matching shell configuration. `cj`
prints source text by default; `-o/--output` writes generated shell source to a
file and creates missing parent directories. It never edits a shell configuration
or profile.

For Bash:

```bash
cj init bash -o ~/.config/cj/init.bash
cj completions bash -o ~/.config/cj/completions.bash

# Add to ~/.bashrc:
source ~/.config/cj/init.bash
source ~/.config/cj/completions.bash
```

For Zsh:

```zsh
cj init zsh -o ~/.config/cj/init.zsh
cj completions zsh -o ~/.config/cj/completions.zsh

# Add to ~/.zshrc:
autoload -Uz compinit && compinit
source ~/.config/cj/init.zsh
source ~/.config/cj/completions.zsh
```

For Nushell:

```nu
cj init nu -o ~/.config/nushell/cj.nu
cj completions nu -o ~/.config/nushell/cj-completions.nu

# Add to config.nu:
use ~/.config/nushell/cj.nu *
use ~/.config/nushell/cj-completions.nu *
```

For PowerShell 7:

```powershell
$CjConfig = Join-Path (Split-Path -Parent $PROFILE) 'cj'
cj init powershell -o (Join-Path $CjConfig 'init.ps1')
cj completions powershell -o (Join-Path $CjConfig 'completions.ps1')

# Add these lines to $PROFILE:
$CjConfig = Join-Path (Split-Path -Parent $PROFILE) 'cj'
. (Join-Path $CjConfig 'init.ps1')
. (Join-Path $CjConfig 'completions.ps1')
```

Restart the shell or source its configuration after making the change.
`pwsh` is accepted as an input alias for `powershell`; generated source and
completion candidates use the canonical `powershell` name.
`cj init` installs the configured directory completion binding by default on
Bash, Zsh, Nushell, and PowerShell. The Bash widget requires Bash 4 or newer;
macOS's system Bash 3 still supports the `cd` wrapper and Tab completion.
PowerShell installs the widget when PSReadLine is available. Use
`cj init <shell> --no-setup-key-binding` to generate the wrapper and completions
without installing the widget.

## Jumping

Once the shell integration is loaded, use `cd` normally:

```console
cd src          # existing directories always use the builtin directly
cd project      # otherwise use the configured resolver (zoxide by default)
cd code         # a configured alias
cd external-ssd # a configured mount
cd top          # top level of the current Git repository
cd origin       # main worktree; "og" is also enabled by default
cd ^^^          # three directories up
cd vvv          # go back three directory-history entries
```

The down ticker uses browser-style directory history: every successful `cd` change
remembers the directory you left, including unrelated paths, aliases, mounts, raw
paths, and zoxide jumps. `cd v` goes back one entry; `cd vvv` goes back three.
Moving up with `cd ^^^` records each traversed parent so `cd vvv` retraces the move.
History keeps the latest 100 departures in the current shell session and resets
when you reload the integration. Existing directories always win over tickers.

Going back consumes entries without adding the current directory. An empty or
insufficient history reports `cj: directory history exhausted`; a missing history
destination reports the shell's directory error. Both leave history and the
current directory unchanged. Failed commands and moves to the current directory
also preserve history. Native escapes such as `builtin cd` or `Set-Location` bypass
history tracking. Regenerate and reload your integration after upgrading to use
this behavior.

Resolver flags make the choice explicit:

```console
cd -z project   # use only zoxide; missing/failing zoxide is an error
cd -Z code      # disable zoxide; retain aliases, mounts, tickers, and Git shortcuts
cd -r code      # treat "code" literally; bypass all cj resolution and zoxide
```

When zoxide is the configured default but is unavailable or cannot find a match,
`cj` falls back to normal literal-path behavior. `cj` invokes the configured zoxide
executable directly; it does not run commands through a shell.

For ordinary resolution, the first match wins: an existing directory, navigation
ticker, alias, mount, Git keyword, then zoxide or the literal fallback. A configured
alias or mount whose destination is unavailable is an error; it does not fall
through to a later resolver.

## Worktrees

List the current repository's worktrees in a table:

```console
cj -w
cj --worktree
```

JSON output and paths relative to the current directory are also available:

```console
cj -w --format json
cj -w --relative
```

Choose a worktree with Tab completion:

```console
cd -jw<Tab>            # complete a worktree path, then press Enter
cd -jw <Tab>           # a space before Tab also works
cd --jump-worktree<Tab> # long form of the same completion trigger
```

`cj -w/--worktree` lists worktrees. The `cd` completion requires the generated
shell integration so the parent shell can perform the directory change.
In Bash, Zsh, Nushell, and PowerShell, pressing Tab
after `-jw` or `--jump-worktree`, with or without trailing whitespace, offers only
this repository's worktree paths through the shell's native completion system. A completion frontend
such as fzf-tab may render those candidates with fzf, but cj does not launch a
nested picker during completion and works without fzf. Choosing a candidate only
inserts its path; press Enter to change directory. After whitespace, Bash, Zsh,
and Nushell keep the flag and complete its literal destination argument; PowerShell
replaces the flag and whitespace together. Pressing Enter with a jump flag but
no destination shows a reminder to use Tab and leaves the directory unchanged.

The standalone `cj -jw` / `cj --jump-worktree` command remains available to print
a worktree path selected through fzf.

The key installed by `cj init` completes a directory from history. Its default
chord is <kbd>Ctrl</kbd>+<kbd>O</kbd> on macOS and Windows, and
<kbd>Alt</kbd>+<kbd>O</kbd> on Linux. Each OS's `key-bindings` entry has an ordered
`behaviors` list, defaulting to `["zoxide", "cj"]`. `zoxide` opens its interactive
directory picker using fzf. `cj` cycles through cj's browser-style directory
history without external dependencies. Both append a safely quoted directory to
the current command line; press Enter to execute it. The key itself never changes
the current directory.

Behaviors run from left to right until one supplies a directory. A missing
dependency or empty candidate source falls through to the next behavior.
Cancelling a picker stops the chain and preserves the command line, cursor, and
current directory; other errors stop with a diagnostic. Reorder the list to
change which behavior is tried first, or use a single entry to select one source.

After a cj history completion, repeated presses cycle from newest to oldest,
then restore the original line and cursor. Editing the line starts a new cycle.
Completion itself does not consume history. Dependency availability is checked
on each invocation, so installing or removing zoxide or fzf needs no regeneration.
The binding preserves zoxide settings such as `_ZO_FZF_OPTS`.

Regenerate shell integration after upgrading to receive the default binding:

```bash
cj init zsh -o ~/.config/cj/init.zsh
source ~/.config/cj/init.zsh
```

To keep the `cd` wrapper and Tab completion without the directory widget, add
`--no-setup-key-binding`. The old `--setup-key-binding` flag is still accepted as
a compatibility no-op; existing initialization commands can keep it or remove it.
The two flags cannot be combined. An empty `behaviors` list in the current OS's
`key-bindings` entry disables the widget in configuration.

## Configuration

The default config path is `${XDG_CONFIG_HOME:-$HOME/.config}/cj/config.toml` on
macOS and Linux. Windows uses `%APPDATA%\cj\config.toml` (while still honoring
`XDG_CONFIG_HOME` when set).
Every section and field is optional. A complete example is:

```toml
[behavior]
default = "zoxide" # or "builtin"

[programs]
zoxide = "zoxide"
fzf = "fzf"

[key-bindings]
macos = { key = "ctrl-o", behaviors = ["zoxide", "cj"] }
linux = { key = "alt-o", behaviors = ["zoxide", "cj"] }
windows = { key = "ctrl-o", behaviors = ["zoxide", "cj"] }

[keywords]
top = ["top"]
main-worktree = ["origin", "og"]

[tickers]
navigate_up = "^"
navigate_down = "v"

[aliases]
code = "~/Git"
downloads = "~/Downloads"

[mounts]
icloud = { provider = "icloud" }
google-work = { path = "~/Library/CloudStorage/GoogleDrive-work@example.com" }
external-ssd = { path = "/Volumes/External SSD" }
onedrive-work = { provider = "onedrive", account = "business" }
```

Program values may be executable names found on `PATH` or explicit executable
paths. Key binding keys accept `ctrl-o`, `alt-o`, or `none`. Behaviors run from
left to right: `zoxide` uses its interactive picker and `cj` cycles through
browser-style directory history. Missing dependencies or an empty candidate source
fall through; cancellation preserves the command line, cursor, and directory,
and other errors stop the chain. Use `["cj", "zoxide"]` to prefer cj,
`["cj"]` or `["zoxide"]` for one source, or `[]` to disable the binding.
Unknown or duplicate behaviors are errors.
The legacy strings (for example, `linux = "alt-o"`) remain valid and use
`["zoxide", "cj"]`; `"none"` disables the binding. Omitted keys retain the
platform default, and omitted behaviors retain the default chain.

Alias and explicit mount paths must be absolute or start with `~/`; Windows also
accepts `~\`. Drive-letter paths and UNC paths are supported. cj expands only that
leading home marker and does not evaluate shell variables, globs, or command
substitutions. Paths containing spaces or single quotes are preserved. A mount must
specify exactly one `path` or provider; supported providers are `icloud`,
`google-drive`, and `onedrive`. OneDrive alone accepts the optional account selector
`personal` or `business`; resolution succeeds only when exactly one matching root is
registered. Alias and mount names may not collide.

Navigation tickers are configurable single characters. Allowed values are `^`,
`v`, `u`, `d`, `j`, and `k`; the intentionally small allowlist excludes shell
operators and other characters that are unsafe to type unquoted. Up and down must
use different values. After changing tickers or key bindings, rerun the matching
`cj init` command and reload the generated file.
The `cd -jw` Tab hook is also part of this generated integration, so regenerate it
after upgrading cj.
Completions contain a deterministic snapshot of configured keywords, aliases,
mounts, and tickers, so regenerate the completion file after changing them.

When migrating from 0.1.0, replace `keywords.tickers = ["^"]` with the new
top-level `[tickers]` section shown above.

Use `-C` or `--config` to select a different file. When generating shell setup,
the selected config path is embedded safely in the generated wrapper:

```bash
cj -C "$HOME/.config/cj/work.toml" init zsh \
  -o "$HOME/.config/cj/init.zsh"
```

There is no configurable `cd` executable: `cd` is a shell builtin. `cj` only prints
the destination, and the generated integration invokes the parent shell's native
location command (`cd` or `Set-Location`).

## Config initialization and mount discovery

Create a new config at the default path, or at the path selected with `-C`:

```console
cj config init
cj -C ~/.config/cj/work.toml config init
```

Initialization never overwrites an existing file. On macOS and Windows, `--preamp`
scans for reachable mounts and adds only unambiguous results as explicit absolute
`path` entries:

```console
cj config init --preamp
```

Discovery checks the standard iCloud Drive directory, Google Drive directories
under `~/Library/CloudStorage/GoogleDrive-*` and `/Volumes/GoogleDrive*`, and mounted
directories under `/Volumes`. It uses the fixed names `icloud` and `google-drive`;
other volume labels are converted to names such as `External SSD` → `external-ssd`.
Ambiguous Google accounts and name collisions are reported and not written.

On Windows, discovery reads the current user's registered cloud sync roots through
the Windows sync-root API and enumerates non-system drive roots. OneDrive personal
and business roots are named separately. Multiple matching accounts, duplicate
volume labels, mapped network drives, and cloud drives with changeable drive letters
are reported but never persisted automatically. Verbose output shows explicit path
snippets so the user can choose one. Discovery does not inspect OneDrive settings,
credentials, account tokens, or Microsoft Graph.

Inspect discoverable mounts without changing the config:

```console
cj mounts scan
cj mounts scan -f json
cj -v mounts scan
```

The default format is a table. JSON is available for tooling, and the global
`-v`/`--verbose` flag includes discovery details such as skipped candidates and
ready-to-copy mount entries.

## License

MIT
