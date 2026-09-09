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
cj init powershell --setup-key-binding -o (Join-Path $CjConfig 'init.ps1')
cj completions powershell -o (Join-Path $CjConfig 'completions.ps1')

# Add these lines to $PROFILE:
$CjConfig = Join-Path (Split-Path -Parent $PROFILE) 'cj'
. (Join-Path $CjConfig 'init.ps1')
. (Join-Path $CjConfig 'completions.ps1')
```

Restart the shell or source its configuration after making the change.
`pwsh` is accepted as an input alias for `powershell`; generated source and
completion candidates use the canonical `powershell` name.
On PowerShell, the optional picker binding is installed through PSReadLine when
that module is available and is otherwise skipped without changing the profile.

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
cd --jump-worktree<Tab> # long form of the same completion trigger
```

`cj -w/--worktree` lists worktrees. The `cd` completion requires the generated
shell integration so the parent shell can perform the directory change.
In Bash, Zsh, Nushell, and PowerShell, pressing Tab
immediately after `-jw` or `--jump-worktree` offers only this repository's
worktree paths through the shell's native completion system. A completion frontend
such as fzf-tab may render those candidates with fzf, but cj does not launch a
nested picker during completion and works without fzf. Choosing a candidate only
inserts its path; press Enter to change directory. Pressing Enter while the jump
token is still present shows a reminder to use Tab and leaves the directory unchanged.

The standalone `cj -jw` / `cj --jump-worktree` command remains available to print
a path selected through fzf; it also powers the optional worktree key binding.

To install an fzf worktree jump key together with the `cd` wrapper, add the setup
flag to shell initialization:

```bash
eval "$(cj init zsh --setup-key-binding)"
```

The default picker key is <kbd>Ctrl</kbd>+<kbd>O</kbd> on macOS and Windows, and
<kbd>Alt</kbd>+<kbd>O</kbd> on Linux. Pressing it invokes the same interactive
worktree picker as `cj -jw` and changes directory to the selected path. The key
binding requires `fzf`; ordinary directory jumps and worktree listing do not.

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
left to right: `zoxide` uses its interactive picker and `cj` uses cj's picker.
An unavailable picker or an empty result falls through to the next behavior;
cancellation leaves the command line unchanged, and other errors stop the chain.
Use `["cj", "zoxide"]` to prefer cj, `["cj"]` or `["zoxide"]` to use one
picker, or `[]` to disable the binding. Unknown or duplicate behaviors are errors.
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
cj -C "$HOME/.config/cj/work.toml" init zsh --setup-key-binding \
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
