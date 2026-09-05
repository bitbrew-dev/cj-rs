mod support;

use std::path::Path;

use serde_json::Value;
use support::{GitFixture, assert_success, cj};

#[test]
fn resolves_top_and_main_worktree_exactly() {
    let fixture = GitFixture::new("git-resolution");

    let top = cj(&fixture.main_nested, fixture.temp.path())
        .arg("top")
        .output()
        .expect("run cj top");
    assert_success(&top);
    assert_eq!(top.stdout, destination(&fixture.main));
    assert!(top.stderr.is_empty());

    let origin = cj(&fixture.linked_nested, fixture.temp.path())
        .arg("origin")
        .output()
        .expect("run cj origin");
    assert_success(&origin);
    assert_eq!(origin.stdout, destination(&fixture.main));
    assert!(origin.stderr.is_empty());
}

#[test]
fn renders_worktrees_as_table_json_and_relative_paths() {
    let fixture = GitFixture::new("git-output");

    let table = cj(&fixture.main_nested, fixture.temp.path())
        .arg("--worktree")
        .output()
        .expect("render worktree table");
    assert_success(&table);
    let table = String::from_utf8(table.stdout).expect("table is UTF-8");
    assert!(
        table
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("PATH"))
    );
    assert!(table.contains(fixture.main.to_str().unwrap()));
    assert!(table.contains(fixture.linked.to_str().unwrap()));
    assert!(table.contains("feature/quoted-path"));

    let json = cj(&fixture.main_nested, fixture.temp.path())
        .args(["--worktree", "--format", "json"])
        .output()
        .expect("render worktree JSON");
    assert_success(&json);
    let rows: Value = serde_json::from_slice(&json.stdout).expect("valid worktree JSON");
    let rows = rows.as_array().expect("worktree JSON array");
    assert_eq!(rows.len(), 2);
    let main = row_for_path(rows, &fixture.main);
    assert_eq!(main["branch"], "main");
    assert_eq!(main["main"], true);
    let linked = row_for_path(rows, &fixture.linked);
    assert_eq!(linked["branch"], "feature/quoted-path");
    assert_eq!(linked["main"], false);

    let relative = cj(&fixture.main_nested, fixture.temp.path())
        .args(["--worktree", "--format", "json", "--relative"])
        .output()
        .expect("render relative worktree JSON");
    assert_success(&relative);
    let rows: Value = serde_json::from_slice(&relative.stdout).expect("valid relative JSON");
    let rows = rows.as_array().expect("relative worktree JSON array");
    assert!(rows.iter().any(|row| row["path"] == ".."));
    let linked_relative = Path::new("../..").join("feature's worktree");
    assert!(
        rows.iter()
            .any(|row| row["path"] == linked_relative.to_str().unwrap())
    );
}

#[test]
fn reports_git_failures_on_stderr() {
    let fixture = GitFixture::new("git-failure");
    let output = cj(fixture.temp.path(), fixture.temp.path())
        .arg("--worktree")
        .output()
        .expect("run cj outside repository");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("cj: git worktree list failed:"));
    assert!(stderr.contains("not a git repository"));
}

fn destination(path: &Path) -> Vec<u8> {
    format!("{}\n", path.display()).into_bytes()
}

fn row_for_path<'a>(rows: &'a [Value], path: &Path) -> &'a Value {
    rows.iter()
        .find(|row| row["path"] == path.to_str().unwrap())
        .expect("worktree row")
}
