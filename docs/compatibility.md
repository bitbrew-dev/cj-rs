# Compatibility from 1.0.0

This contract applies starting with cj 1.0.0. Throughout 1.x, existing documented
commands and valid configuration retain their meaning. New features can be added,
and bugs can be corrected to match the documented behavior. An incompatible change
to the public contract requires a major release and migration notes, following
[Semantic Versioning](https://semver.org/).

## Public commands and configuration

The public interface consists of the commands and flags in `cj --help` and the
usage documented in the [README](../README.md). This includes their accepted
arguments, resolver controls, and the distinction between printing a destination
with `cj` and changing the parent shell's directory with the generated `cd` wrapper.
Documented aliases such as `pwsh` and the compatibility flag
`--setup-key-binding` remain accepted throughout 1.x.

Existing documented TOML sections, field names, accepted values, and defaults remain
supported. This includes configuration path selection, the legacy string form of
key bindings, and the legacy `one-drive` spelling for the `onedrive` provider.
New optional settings may be added. Unknown fields are rejected, so a configuration
using a new setting may require the version that introduced it; compatibility does
not mean that older binaries understand newer settings.

For ordinary resolution, existing directories take precedence over navigation
tickers, aliases, mounts, and Git keywords, in that order. The configured resolver
then supplies zoxide or literal-path fallback. Explicit `-r`, `-z`, and `-Z` modes
keep their documented overrides. A configured alias or mount that cannot be reached
remains an error rather than silently resolving to another destination.

Navigation supports both repeated tickers and positive decimal counts: `^^` and
`^2` are equivalent, as are `vvv` and `v3`, including with configured tickers.
Upward counts cap at the filesystem root; backward counts exceeding available
history fail without changing the directory or history. Count limits and the
directory-first rule are documented in the README.

## Generated shell integration

The documented `cd` wrapper, directory history, Tab completion, and optional key
binding behavior are public across Bash, Zsh, Nushell, and PowerShell, subject to
the shell requirements in the README. Completion inserts a candidate; execution
performs the directory change. Native shell escapes bypass cj's history tracking.

In particular, bare `cd -jw` and `cd --jump-worktree` jump to the primary worktree,
regardless of the branch checked out there. Tab, with or without a preceding
space, offers worktree candidates. Standalone `cj -jw` remains an fzf picker that
prints the selected path. These operations do not switch Git branches.

After upgrading the binary, regenerate both `cj init <shell>` and
`cj completions <shell>` output using that binary and your chosen `-C` configuration,
then reload them in the matching shell or restart it. The README contains the
commands for each shell. Also regenerate after changing settings captured in the
generated files, such as tickers, key bindings, or completion names. Reloading the
integration resets the session's directory history.

Generated files and the binary are a matched pair. Their source text, helper
function names, private variables, and internal protocols may change within 1.x.
Do not call those helpers directly, edit the generated files as an extension API,
or rely on mixing files from one version with a different binary.

## Output for scripts

A successful destination-resolution command writes the path followed by one LF
to stdout. The path is data, not shell code: it is not shell-quoted and can be
relative. Quote it when passing it to the shell's directory command. On Unix,
path output can contain non-UTF-8 bytes and embedded or trailing newlines; remove
only the final output LF when preserving the exact path.

Cancelling the standalone worktree picker, or getting no match, succeeds with an
empty selection (one output LF). Check that a destination was returned before
changing directory. A successful resolution alone does not guarantee that a
literal fallback path exists; the shell performs the directory change.

Scripts may rely on exit status zero for success and nonzero for failure, and on
diagnostics going to stderr rather than being mixed into path or JSON output.
Exact diagnostic wording and individual nonzero status values are not stable APIs.

Use `--format json` for worktree listings and mount discovery. Existing fields,
their types, nullability, and meanings are stable. Consumers must ignore unknown
fields, tolerate new `source` and `status` strings, and avoid depending on JSON
whitespace, object key order, or array order.

### Worktree JSON

`cj --worktree --format json` returns an array of objects:

| Fields | Type and meaning |
| --- | --- |
| `path` | String containing the worktree path; `--relative` makes it relative to the current directory when possible. |
| `branch`, `head` | Branch name without `refs/heads/`, and full commit ID; each is a string or `null` when absent. |
| `main`, `bare`, `detached`, `locked`, `prunable` | Boolean state flags. `main` identifies the primary worktree, not a branch named `main`. |
| `lock_reason`, `prune_reason` | String or `null`; a true state flag can have no reason. |

Worktree JSON requires Unicode paths; an unrepresentable path is an error rather
than a silently changed destination. Path separators and absolute path syntax
follow the platform, so compare paths using platform-aware operations.

### Mount scan JSON

`cj mounts scan --format json` returns an object with these arrays:

| Field | Entries |
| --- | --- |
| `mounts` | Objects with string fields `name`, `source`, and `path` for ready mounts. |
| `skipped` | Objects with string fields `name`, `source`, `status`, and `reason`. With `--verbose`, each also includes `candidates`, an array of path strings. |
| `warnings` | Diagnostic strings. |

The text in `reason` and `warnings` is for people and can change; use structural
fields for automation. Discovered paths and the set of available mounts depend on
the current machine and connected storage.

## Outside the public contract

Human-readable tables, help layout, picker appearance, completion ordering, and
exact generated source can evolve within 1.x. Undocumented flags (including
`--internal-*`), `CJ_INTERNAL_*` environment variables, and generated helper names
are private implementation details. Rust modules are internal to the executable;
the package does not promise a Rust library API.

## Release convention

This repository uses the `incompat` commit type to request a major release, `feat`
for a minor release, and `fix` for a patch release. Every intentional incompatible
public change must use `incompat: ...` or `incompat(scope): ...` and explain the
migration. The commit-message hook requires the scoped form, for example
`incompat(release): establish the v1 compatibility contract`. It is a commit type,
not a scope on `feat` or `fix`.

The current release rules map a breaking-change marker alone to a minor release;
`!` or a `BREAKING CHANGE` footer therefore does not replace the required
`incompat` type. The first v1 contract commit uses `incompat` to request 1.0.0.
Version files and tags are produced by the maintainer-triggered release workflow.
