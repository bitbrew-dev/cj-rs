use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::cli::OutputFormat;

#[derive(Debug, PartialEq)]
pub struct Worktree {
    path: PathBuf,
    head: Option<String>,
    branch: Option<String>,
    bare: bool,
    detached: bool,
    locked: Option<Option<String>>,
    prunable: Option<Option<String>>,
}

pub fn list() -> Result<Vec<Worktree>, String> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            "current directory is not in a Git repository".into()
        } else {
            format!("git worktree list failed: {detail}")
        });
    }
    parse(&output.stdout)
}

pub fn render(
    worktrees: &[Worktree],
    format: OutputFormat,
    relative: bool,
    cwd: &Path,
) -> Result<String, String> {
    let rows = worktrees
        .iter()
        .enumerate()
        .map(|(index, worktree)| worktree.row(index == 0, relative, cwd))
        .collect::<Vec<_>>();

    match format {
        OutputFormat::Table => Ok(render_table(&rows)),
        OutputFormat::Json => serde_json::to_string_pretty(&rows)
            .map_err(|error| format!("cannot serialize worktrees: {error}")),
    }
}

pub fn pick(worktrees: &[Worktree], fzf: &Path) -> Result<Option<PathBuf>, String> {
    let mut input = Vec::new();
    for (index, worktree) in worktrees.iter().enumerate() {
        let row = worktree.row(index == 0, false, Path::new(""));
        write!(
            input,
            "{index}\t{}\t{}\t{}\0",
            row.path,
            row.branch.as_deref().unwrap_or("-"),
            state(&row)
        )
        .map_err(|error| format!("cannot prepare fzf input: {error}"))?;
    }

    let mut child = Command::new(fzf)
        .args([
            "--read0",
            "--print0",
            "--delimiter=\\t",
            "--with-nth=2..",
            "--nth=2..",
            "--height=40%",
            "--layout=reverse",
            "--border",
            "--prompt=cj worktree> ",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("fzf executable not found: {}", fzf.display())
            } else {
                format!("cannot run fzf: {error}")
            }
        })?;
    child
        .stdin
        .take()
        .ok_or("cannot open fzf input")?
        .write_all(&input)
        .map_err(|error| format!("cannot write fzf input: {error}"))?;

    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot wait for fzf: {error}"))?;
    if matches!(output.status.code(), Some(1) | Some(130)) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(format!("fzf exited with {}", output.status));
    }
    let selection = output.stdout.split(|byte| *byte == 0).next().unwrap_or(&[]);
    let index = selection
        .split(|byte| *byte == b'\t')
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or("fzf returned an invalid worktree selection")?;
    worktrees
        .get(index)
        .map(|worktree| Some(worktree.path.clone()))
        .ok_or("fzf returned an unknown worktree selection".into())
}

fn parse(output: &[u8]) -> Result<Vec<Worktree>, String> {
    let mut worktrees = Vec::new();
    let mut current = None;

    for raw_field in output.split(|byte| *byte == 0) {
        if raw_field.is_empty() {
            if let Some(worktree) = current.take() {
                worktrees.push(worktree);
            }
            continue;
        }
        let field = std::str::from_utf8(raw_field)
            .map_err(|_| "git returned a non-UTF-8 worktree record")?;
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));

        if key == "worktree" {
            if let Some(worktree) = current.replace(Worktree::new(value)?) {
                worktrees.push(worktree);
            }
            continue;
        }

        let worktree = current
            .as_mut()
            .ok_or_else(|| format!("git returned {key} before a worktree path"))?;
        match key {
            "HEAD" => worktree.head = Some(value.into()),
            "branch" => worktree.branch = Some(strip_branch_prefix(value).into()),
            "bare" => worktree.bare = true,
            "detached" => worktree.detached = true,
            "locked" => worktree.locked = Some(optional_reason(value)),
            "prunable" => worktree.prunable = Some(optional_reason(value)),
            _ => {}
        }
    }
    if let Some(worktree) = current {
        worktrees.push(worktree);
    }
    if worktrees.is_empty() {
        return Err("git did not return any worktrees".into());
    }
    Ok(worktrees)
}

impl Worktree {
    fn new(path: &str) -> Result<Self, String> {
        if path.is_empty() {
            return Err("git returned an empty worktree path".into());
        }
        Ok(Self {
            path: path.into(),
            head: None,
            branch: None,
            bare: false,
            detached: false,
            locked: None,
            prunable: None,
        })
    }

    fn row(&self, main: bool, relative: bool, cwd: &Path) -> WorktreeRow {
        let path = if relative {
            relative_path(&self.path, cwd)
        } else {
            self.path.clone()
        };
        WorktreeRow {
            path: path.to_string_lossy().into_owned(),
            branch: self.branch.clone(),
            head: self.head.clone(),
            main,
            bare: self.bare,
            detached: self.detached,
            locked: self.locked.is_some(),
            lock_reason: self.locked.clone().flatten(),
            prunable: self.prunable.is_some(),
            prune_reason: self.prunable.clone().flatten(),
        }
    }
}

#[derive(Serialize)]
struct WorktreeRow {
    path: String,
    branch: Option<String>,
    head: Option<String>,
    main: bool,
    bare: bool,
    detached: bool,
    locked: bool,
    lock_reason: Option<String>,
    prunable: bool,
    prune_reason: Option<String>,
}

fn render_table(rows: &[WorktreeRow]) -> String {
    let mut values = Vec::with_capacity(rows.len());
    for row in rows {
        let branch = row.branch.as_deref().unwrap_or("-");
        let head = row
            .head
            .as_deref()
            .map(|head| &head[..head.len().min(8)])
            .unwrap_or("-");
        values.push((row.path.as_str(), branch, head, state(row)));
    }

    let path_width = values
        .iter()
        .map(|row| row.0.chars().count())
        .chain([4])
        .max()
        .unwrap_or(4);
    let branch_width = values
        .iter()
        .map(|row| row.1.chars().count())
        .chain([6])
        .max()
        .unwrap_or(6);

    let mut lines = vec![format!(
        "{:<path_width$}  {:<branch_width$}  {:<8}  STATE",
        "PATH", "BRANCH", "HEAD"
    )];
    lines.extend(values.into_iter().map(|(path, branch, head, state)| {
        format!("{path:<path_width$}  {branch:<branch_width$}  {head:<8}  {state}")
    }));
    lines.join("\n")
}

fn state(row: &WorktreeRow) -> String {
    let mut states = Vec::new();
    if row.main {
        states.push("main");
    }
    if row.bare {
        states.push("bare");
    }
    if row.detached {
        states.push("detached");
    }
    if row.locked {
        states.push("locked");
    }
    if row.prunable {
        states.push("prunable");
    }
    if states.is_empty() {
        states.push("linked");
    }
    states.join(",")
}

fn relative_path(path: &Path, base: &Path) -> PathBuf {
    let path_components = path.components().collect::<Vec<_>>();
    let base_components = base.components().collect::<Vec<_>>();
    let common = path_components
        .iter()
        .zip(&base_components)
        .take_while(|(left, right)| left == right)
        .count();

    if common == 0
        || base_components[common..]
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return path.to_path_buf();
    }

    let mut relative = PathBuf::new();
    for _ in &base_components[common..] {
        relative.push("..");
    }
    for component in &path_components[common..] {
        relative.push(component.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    relative
}

fn optional_reason(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.into())
}

fn strip_branch_prefix(branch: &str) -> &str {
    branch.strip_prefix("refs/heads/").unwrap_or(branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORCELAIN: &[u8] = b"worktree /repo\0HEAD 0123456789abcdef\0branch refs/heads/main\0\0worktree /repo/feature\0HEAD fedcba9876543210\0detached\0locked reason\0\0";

    #[test]
    fn parses_porcelain_records() {
        let worktrees = parse(PORCELAIN).unwrap();
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[0].branch.as_deref(), Some("main"));
        assert!(worktrees[1].detached);
        assert_eq!(worktrees[1].locked, Some(Some("reason".into())));
    }

    #[test]
    fn renders_table_with_short_heads_and_state() {
        let table = render(
            &parse(PORCELAIN).unwrap(),
            OutputFormat::Table,
            false,
            Path::new("/repo"),
        )
        .unwrap();
        assert!(table.contains("/repo          main    01234567  main"));
        assert!(table.contains("/repo/feature  -       fedcba98  detached,locked"));
    }

    #[test]
    fn renders_json_with_relative_paths() {
        let json = render(
            &parse(PORCELAIN).unwrap(),
            OutputFormat::Json,
            true,
            Path::new("/repo/subdir"),
        )
        .unwrap();
        assert!(json.contains("\"path\": \"..\""));
        assert!(json.contains("\"path\": \"../feature\""));
        assert!(json.contains("\"main\": true"));
    }

    #[test]
    fn identical_path_is_dot() {
        assert_eq!(
            relative_path(Path::new("/repo"), Path::new("/repo")),
            Path::new(".")
        );
    }

    #[test]
    fn missing_fzf_has_a_clear_error() {
        let worktrees = parse(PORCELAIN).unwrap();
        let error = pick(&worktrees, Path::new("/definitely/missing/fzf")).unwrap_err();
        assert!(error.contains("fzf executable not found"));
    }
}
