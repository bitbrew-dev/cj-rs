//! Runtime launcher for the snapshotted interactive zoxide configuration.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Private stdout protocol separates fallback from cancellation without changing
/// the normal CLI's error handling. Stderr remains attached to the foreground
/// terminal while zoxide owns the interactive picker.
pub fn query_zoxide(zoxide: &Path, fzf: &Path) -> Result<Vec<u8>, String> {
    let Some(zoxide) = executable(zoxide) else {
        return Ok(format!(
            "unavailable\nzoxide executable not found: {}",
            zoxide.display()
        )
        .into_bytes());
    };
    let Some(fzf) = executable(fzf) else {
        return Ok(
            format!("unavailable\nfzf executable not found: {}", fzf.display()).into_bytes(),
        );
    };
    let candidates = Command::new(&zoxide)
        .args(["query", "--list"])
        .output()
        .map_err(|error| format!("cannot query zoxide: {error}"))?;
    if !candidates.status.success() {
        return Err(format!(
            "zoxide query failed ({}): {}",
            candidates.status,
            String::from_utf8_lossy(&candidates.stderr).trim()
        ));
    }
    if candidates.stdout.is_empty() {
        return Ok(b"empty\n".to_vec());
    }
    let shim = FzfAlias::new(&fzf)?;
    let mut paths = vec![shim.0.clone()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path =
        std::env::join_paths(paths).map_err(|error| format!("cannot set picker PATH: {error}"))?;
    let output = Command::new(zoxide)
        .args(["query", "--interactive"])
        .env("PATH", path)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("cannot launch interactive zoxide query: {error}"))?;
    if output.status.code() == Some(130) {
        return Ok(b"cancelled\n".to_vec());
    }
    if !output.status.success() {
        return Err(format!(
            "interactive zoxide query failed ({})",
            output.status
        ));
    }
    let target = crate::path_bytes::without_line_ending(&output.stdout);
    if target.is_empty() {
        // No selection after opening the picker must not run the next behavior.
        return Ok(b"cancelled\n".to_vec());
    }
    if target.contains(&0) {
        return Err("interactive zoxide returned a path containing NUL".into());
    }
    let mut result = b"selected\n".to_vec();
    result.extend_from_slice(target);
    Ok(result)
}

fn executable(program: &Path) -> Option<PathBuf> {
    let candidates = if program.components().count() > 1 || program.is_absolute() {
        vec![program.to_path_buf()]
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|directory| directory.join(program))
            .collect()
    };
    for candidate in candidates {
        #[cfg(windows)]
        let candidate = if candidate.extension().is_none() {
            candidate.with_extension("exe")
        } else {
            candidate
        };
        let Ok(metadata) = candidate.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        if let Ok(path) = dunce::canonicalize(candidate) {
            return Some(path);
        }
    }
    None
}

struct FzfAlias(PathBuf);
impl FzfAlias {
    fn new(fzf: &Path) -> Result<Self, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let directory =
                std::env::temp_dir().join(format!("cj-fzf-{}-{id}", std::process::id()));
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            let mut builder = builder;
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("cannot create fzf launcher directory: {error}")),
            }
            let alias = Self(directory);
            #[cfg(unix)]
            let result = std::os::unix::fs::symlink(fzf, alias.0.join("fzf"));
            #[cfg(not(unix))]
            let result = std::fs::hard_link(fzf, alias.0.join("fzf.exe"))
                .or_else(|_| std::fs::copy(fzf, alias.0.join("fzf.exe")).map(|_| ()));
            result.map_err(|error| format!("cannot create fzf launcher: {error}"))?;
            return Ok(alias);
        }
        Err("cannot allocate fzf launcher directory".into())
    }
}
impl Drop for FzfAlias {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
