use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::config::{self, Config, MountSpec};

pub fn create(
    explicit_path: Option<&Path>,
    discovered_mounts: impl IntoIterator<Item = (String, PathBuf)>,
) -> Result<PathBuf, String> {
    let path = explicit_path
        .map(PathBuf::from)
        .or_else(config::default_config_path)
        .ok_or("cannot determine config path; set XDG_CONFIG_HOME or HOME")?;
    let mut config = Config::default();
    config
        .mounts
        .extend(discovered_mounts.into_iter().map(|(name, path)| {
            (
                name,
                MountSpec {
                    path: Some(path),
                    provider: None,
                },
            )
        }));
    config.validate()?;
    let contents = toml::to_string_pretty(&config)
        .map_err(|error| format!("cannot serialize config: {error}"))?;
    write_atomic_noclobber(&path, contents.as_bytes())?;
    Ok(path)
}

static NEXT_TEMP_ID: AtomicUsize = AtomicUsize::new(0);

fn write_atomic_noclobber(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "cannot create config directory {}: {error}",
            parent.display()
        )
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("invalid config path: {}", path.display()))?
        .to_string_lossy();

    let (temporary_path, mut file) = loop {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{file_name}.tmp-{}-{id}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("cannot create temporary config file: {error}")),
        }
    };
    let cleanup = TemporaryFile(temporary_path);
    file.write_all(contents)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("cannot write config {}: {error}", path.display()))?;
    drop(file);
    fs::hard_link(&cleanup.0, path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            format!("config already exists: {}", path.display())
        } else {
            format!(
                "cannot install config {} atomically: {error}",
                path.display()
            )
        }
    })?;
    Ok(())
}

struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cj-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn creates_complete_config_without_overwriting() {
        let root = root("config-create");
        let destination = root.join("nested/config.toml");
        let mount = root.join("External SSD's files");
        fs::create_dir_all(&mount).unwrap();

        assert_eq!(
            create(Some(&destination), [("external-ssd".into(), mount.clone())]),
            Ok(destination.clone())
        );
        let original = fs::read(&destination).unwrap();
        let created: Config = toml::from_slice(&original).unwrap();
        assert_eq!(created.mounts["external-ssd"].path.as_ref(), Some(&mount));

        assert!(
            create(Some(&destination), [])
                .unwrap_err()
                .contains("already exists")
        );
        assert_eq!(fs::read(&destination).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_creation_has_one_winner() {
        let root = root("config-race");
        let destination = root.join("config.toml");
        let results = (0..2)
            .map(|_| {
                let destination = destination.clone();
                std::thread::spawn(move || {
                    create(Some(&destination), std::iter::empty::<(String, PathBuf)>())
                })
            })
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result
                    .as_ref()
                    .is_err_and(|error| error.contains("already exists")))
                .count(),
            1
        );
        let created: Config = toml::from_str(&fs::read_to_string(&destination).unwrap()).unwrap();
        assert_eq!(created, Config::default());
        fs::remove_dir_all(root).unwrap();
    }
}
