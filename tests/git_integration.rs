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
    assert_path_output(&top.stdout, &fixture.main);
    assert!(top.stderr.is_empty());

    let origin = cj(&fixture.linked_nested, fixture.temp.path())
        .arg("origin")
        .output()
        .expect("run cj origin");
    assert_success(&origin);
    assert_path_output(&origin.stdout, &fixture.main);
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
    assert_rendered_path(&table, &fixture.main);
    assert_rendered_path(&table, &fixture.linked);
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
    let linked_relative = Path::new("..").join("..").join("feature's worktree");
    assert!(rows.iter().any(|row| {
        row["path"]
            .as_str()
            .is_some_and(|path| path_eq(path, &linked_relative))
    }));
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

fn assert_path_output(output: &[u8], expected: &Path) {
    let actual = std::str::from_utf8(output)
        .expect("path output is UTF-8")
        .strip_suffix('\n')
        .expect("path output ends with a newline");
    assert!(path_eq(actual, expected), "path: {actual:?}");
}

fn assert_rendered_path(output: &str, expected: &Path) {
    let native = expected.to_string_lossy();
    let git_style = native.replace('\\', "/");
    assert!(
        output.contains(native.as_ref()) || output.contains(&git_style),
        "missing path {expected:?}"
    );
}

fn path_eq(actual: &str, expected: &Path) -> bool {
    let expected = expected.to_string_lossy();
    if cfg!(windows) {
        actual
            .replace('\\', "/")
            .eq_ignore_ascii_case(&expected.replace('\\', "/"))
    } else {
        actual == expected
    }
}

fn row_for_path<'a>(rows: &'a [Value], path: &Path) -> &'a Value {
    rows.iter()
        .find(|row| {
            row["path"]
                .as_str()
                .is_some_and(|value| path_eq(value, path))
        })
        .expect("worktree row")
}
