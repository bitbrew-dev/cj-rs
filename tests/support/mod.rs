#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP_ID: AtomicUsize = AtomicUsize::new(0);

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(label: &str) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("cj-{label}-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("create test directory");
        let path = fs::canonicalize(path).expect("canonicalize test directory");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub struct GitFixture {
    pub temp: TempDir,
    pub main: PathBuf,
    pub linked: PathBuf,
    pub main_nested: PathBuf,
    pub linked_nested: PathBuf,
}

impl GitFixture {
    pub fn new(label: &str) -> Self {
        let temp = TempDir::new(label);
        let root = temp.path().join("repository fixture's root");
        let main = root.join("main repo");
        let linked = root.join("feature's worktree");
        fs::create_dir_all(&main).expect("create main repository");

        git(&main, ["init"]);
        git(&main, ["config", "user.name", "CJ Tests"]);
        git(&main, ["config", "user.email", "cj-tests@example.invalid"]);
        git(&main, ["config", "commit.gpgSign", "false"]);
        git(&main, ["commit", "--allow-empty", "-m", "initial"]);
        git(&main, ["branch", "-M", "main"]);

        let output = git_command(&main)
            .args([OsStr::new("worktree"), OsStr::new("add"), OsStr::new("-b")])
            .arg("feature/quoted-path")
            .arg(&linked)
            .output()
            .expect("run git worktree add");
        assert_success(&output);

        let main_nested = main.join("nested dir's child");
        let linked_nested = linked.join("nested dir's child");
        fs::create_dir_all(&main_nested).expect("create main nested directory");
        fs::create_dir_all(&linked_nested).expect("create linked nested directory");

        Self {
            temp,
            main,
            linked,
            main_nested,
            linked_nested,
        }
    }
}

pub fn cj(cwd: &Path, isolated_root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cj"));
    command
        .current_dir(cwd)
        .env("HOME", isolated_root.join("home"))
        .env("XDG_CONFIG_HOME", isolated_root.join("config"))
        .env("GIT_CEILING_DIRECTORIES", isolated_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR");
    command
}

pub fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "status: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git<const N: usize>(cwd: &Path, args: [&str; N]) {
    let output = git_command(cwd).args(args).output().expect("run git");
    assert_success(&output);
}

fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(cwd)
        .args(["-c", "core.hooksPath=/dev/null"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
    command
}
