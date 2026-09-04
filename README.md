# cj

`cj` is a small shell companion for jumping between useful directories. It adds
Git-aware shortcuts, optional zoxide resolution, and an fzf worktree picker while
leaving the final directory change to your shell's real `cd` builtin.

## Install

```console
cargo install cj-rs
```

`cj` supports Bash and Zsh. Add one of these lines to your shell configuration:

```bash
# Bash: ~/.bashrc
eval "$(cj init bash)"

# Zsh: ~/.zshrc
eval "$(cj init zsh)"
```

Restart the shell or source its configuration after making the change.

## Jumping

Once the shell integration is loaded, use `cd` normally:

```console
cd src          # existing directories always use the builtin directly
cd project      # otherwise use the configured resolver (zoxide by default)
cd top          # top level of the current Git repository
cd origin       # main worktree; "og" is also enabled by default
cd ^^^          # three directories up
cd vvv          # three levels back down along the remembered route
```

Downward navigation is browser-style: moving up remembers the directory you left,
and the down ticker walks back toward it one level at a time. A different successful
directory change clears that route. Existing directories always win over tickers.

Resolver flags make the choice explicit:

```console
cd -z project   # require zoxide; missing/failing zoxide is an error
cd -Z top       # disable zoxide while retaining cj shortcuts
cd -r path      # treat path literally; bypass shortcuts and zoxide
```

When zoxide is the configured default but is unavailable or cannot find a match,
`cj` falls back to normal literal-path behavior. `cj` invokes the configured zoxide
executable directly; it does not run commands through a shell.

## Worktrees

List the current repository's worktrees in a table:

```console
cj --worktree
```

JSON output and paths relative to the current directory are also available:

```console
cj -w --format json
cj -w --relative
```

To install an fzf worktree picker together with the `cd` wrapper, add the setup
flag to shell initialization:

```bash
eval "$(cj init zsh --setup-key-binding)"
```

The default picker key is <kbd>Ctrl</kbd>+<kbd>O</kbd> on macOS and
<kbd>Alt</kbd>+<kbd>O</kbd> on Linux. The picker requires `fzf`; ordinary jumps and
worktree listing do not.

## Configuration

The default config path is `${XDG_CONFIG_HOME:-$HOME/.config}/cj/config.toml`.
Every section and field is optional. The complete defaults are:

```toml
[behavior]
default = "zoxide" # or "builtin"

[programs]
zoxide = "zoxide"
fzf = "fzf"

[key-bindings]
macos = "ctrl-o"
linux = "alt-o"

[keywords]
top = ["top"]
main-worktree = ["origin", "og"]

[tickers]
navigate_up = "^"
navigate_down = "v"
```

Program values may be executable names found on `PATH` or explicit executable
paths. Key bindings accept `ctrl-o`, `alt-o`, or `none`.

Navigation tickers are configurable single characters. Allowed values are `^`,
`v`, `u`, `d`, `j`, and `k`; the intentionally small allowlist excludes shell
operators and other characters that are unsafe to type unquoted. Up and down must
use different values. After changing tickers or key bindings, regenerate the shell
integration by starting a new shell or sourcing its configuration again.

When migrating from 0.1.0, replace `keywords.tickers = ["^"]` with the new
top-level `[tickers]` section shown above.

Use `-C` or `--config` to select a different file. When generating shell setup,
the selected config path is embedded safely in the generated wrapper:

```bash
eval "$(cj -C "$HOME/.config/cj/work.toml" init zsh --setup-key-binding)"
```

There is no configurable `cd` executable: `cd` is a shell builtin. `cj` only prints
the destination, and the generated wrapper calls `builtin cd` in the parent shell.

## License

MIT
