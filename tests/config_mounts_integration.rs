mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use support::{TempDir, assert_success, cj};

struct Fixture {
    temp: TempDir,
    discovery: PathBuf,
    cwd: PathBuf,
    config: PathBuf,
    alias: PathBuf,
    mount: PathBuf,
    missing_alias: PathBuf,
    missing_mount: PathBuf,
    icloud: PathBuf,
    volume: PathBuf,
    google: [PathBuf; 2],
}

impl Fixture {
    fn new(label: &str) -> Self {
        let temp = TempDir::new(label);
        let discovery = temp.path().join("discovery root's fixture");
        let home = discovery.join("home");
        let cwd = temp.path().join("working directory");
        let alias = home.join("Code Projects' Archive");
        let mount = discovery.join("manual mounts/External SSD's Files");
        let missing_alias = home.join("Missing Alias");
        let missing_mount = discovery.join("manual mounts/Missing Drive");
        let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
        let volume = discovery.join("Volumes/External SSD's Disk");
        let cloud_storage = home.join("Library/CloudStorage");
        let google = [
            cloud_storage.join("GoogleDrive-one@example.com"),
            cloud_storage.join("GoogleDrive-two@example.com"),
        ];
        for path in [
            &cwd, &alias, &mount, &icloud, &volume, &google[0], &google[1],
        ] {
            fs::create_dir_all(path).expect("create fixture directory");
        }

        let config = temp.path().join("config with spaces/config.toml");
        fs::create_dir_all(config.parent().unwrap()).expect("create config directory");
        fs::write(
            &config,
            format!(
                "[behavior]\ndefault = \"builtin\"\n\
                 [aliases]\ncode = \"{}\"\nmissing-alias = \"{}\"\n\
                 [mounts.external-ssd]\npath = \"{}\"\n\
                 [mounts.missing-mount]\npath = \"{}\"\n\
                 [mounts.icloud]\nprovider = \"icloud\"\n",
                toml_path(&alias),
                toml_path(&missing_alias),
                toml_path(&mount),
                toml_path(&missing_mount),
            ),
        )
        .expect("write fixture config");

        Self {
            temp,
            discovery,
            cwd,
            config,
            alias,
            mount,
            missing_alias,
            missing_mount,
            icloud,
            volume,
            google,
        }
    }

    fn command(&self) -> Command {
        let mut command = cj(&self.cwd, &self.discovery);
        command.env("CJ_INTERNAL_DISCOVERY_ROOT", &self.discovery);
        command
    }

    fn configured_command(&self) -> Command {
        let mut command = self.command();
        command.arg("-C").arg(&self.config);
        command
    }
}

#[test]
fn resolves_aliases_and_explicit_mounts_with_resolver_controls() {
    let fixture = Fixture::new("configured-destinations");

    for (target, destination) in [
        ("code", &fixture.alias),
        ("external-ssd", &fixture.mount),
        ("icloud", &fixture.icloud),
    ] {
        let output = fixture
            .configured_command()
            .arg(target)
            .output()
            .expect("resolve configured destination");
        assert_destination(&output, destination);

        let no_zoxide = fixture
            .configured_command()
            .args(["-Z", target])
            .output()
            .expect("resolve configured destination without zoxide");
        assert_destination(&no_zoxide, destination);
    }

    let raw = fixture
        .configured_command()
        .args(["-r", "code"])
        .output()
        .expect("resolve raw target");
    assert_success(&raw);
    assert_eq!(raw.stdout, b"code\n");
    assert!(raw.stderr.is_empty());

    for (target, kind, missing) in [
        ("missing-alias", "alias", &fixture.missing_alias),
        ("missing-mount", "mount", &fixture.missing_mount),
    ] {
        let output = fixture
            .configured_command()
            .arg(target)
            .output()
            .expect("resolve missing configured destination");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("error is UTF-8");
        assert!(stderr.contains(&format!("{kind} \"{target}\" is not reachable")));
        assert!(stderr.contains(missing.to_str().unwrap()));
    }
}

#[test]
fn config_init_refuses_to_overwrite_an_existing_file() {
    let fixture = Fixture::new("config-init");
    let destination = fixture.temp.path().join("new config/config.toml");

    let created = fixture
        .command()
        .arg("-C")
        .arg(&destination)
        .args(["config", "init"])
        .output()
        .expect("initialize config");
    assert_success(&created);
    assert_eq!(created.stdout, path_message("created", &destination));
    assert!(created.stderr.is_empty());
    let original = fs::read(&destination).expect("read initialized config");
    toml::from_slice::<toml::Value>(&original).expect("initialized config is valid TOML");

    let repeated = fixture
        .command()
        .arg("-C")
        .arg(&destination)
        .args(["config", "init"])
        .output()
        .expect("repeat config initialization");
    assert_eq!(repeated.status.code(), Some(2));
    assert!(repeated.stdout.is_empty());
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("config already exists"));
    assert_eq!(fs::read(destination).unwrap(), original);
}

#[test]
fn preamp_persists_ready_mounts_and_skips_ambiguous_google_accounts() {
    let fixture = Fixture::new("config-preamp");
    let destination = fixture.temp.path().join("preamp/config.toml");
    let output = fixture
        .command()
        .arg("-C")
        .arg(&destination)
        .args(["config", "init", "--preamp"])
        .output()
        .expect("initialize preamp config");

    assert_success(&output);
    assert_eq!(output.stdout, path_message("created", &destination));
    let stderr = String::from_utf8(output.stderr).expect("diagnostics are UTF-8");
    assert_concise_google_warning(&stderr, &fixture);

    let document: toml::Value =
        toml::from_str(&fs::read_to_string(destination).unwrap()).expect("valid preamp config");
    let mounts = document["mounts"].as_table().expect("mount table");
    assert_eq!(mounts["icloud"]["path"].as_str(), fixture.icloud.to_str());
    assert!(mounts.values().any(|mount| {
        let mount = mount.as_table().expect("mount entry");
        mount.get("path").and_then(toml::Value::as_str) == fixture.volume.to_str()
            && mount.get("provider").is_none()
    }));
    assert!(!mounts.contains_key("google-drive"));
    assert!(!mounts.values().any(|mount| {
        fixture
            .google
            .iter()
            .any(|path| mount.get("path").and_then(toml::Value::as_str) == path.to_str())
    }));
}

#[test]
fn mount_scan_separates_table_diagnostics_and_controls_json_detail() {
    let fixture = Fixture::new("mount-scan");

    let concise = fixture
        .command()
        .args(["mounts", "scan"])
        .output()
        .expect("scan mounts");
    assert_success(&concise);
    let table = String::from_utf8(concise.stdout).expect("table is UTF-8");
    assert!(table.lines().next().unwrap().starts_with("NAME"));
    assert!(table.contains(fixture.icloud.to_str().unwrap()));
    assert!(table.contains(fixture.volume.to_str().unwrap()));
    assert!(table.contains("google-drive"));
    assert!(!table.contains(fixture.google[0].to_str().unwrap()));
    let concise_stderr = String::from_utf8(concise.stderr).expect("diagnostics are UTF-8");
    assert_concise_google_warning(&concise_stderr, &fixture);

    let verbose = fixture
        .command()
        .args(["--verbose", "mounts", "scan"])
        .output()
        .expect("scan mounts verbosely");
    assert_success(&verbose);
    assert_eq!(String::from_utf8(verbose.stdout).unwrap(), table);
    let verbose_stderr = String::from_utf8(verbose.stderr).expect("diagnostics are UTF-8");
    assert!(verbose_stderr.contains("Skipped:"));
    assert!(verbose_stderr.contains("google-drive"));
    assert!(!verbose_stderr.contains("rerun with --verbose"));
    for candidate in &fixture.google {
        assert!(verbose_stderr.contains(candidate.to_str().unwrap()));
    }
    assert!(verbose_stderr.contains("Suggested config:"));
    assert!(verbose_stderr.contains("[mounts.icloud]"));

    let concise_json = fixture
        .command()
        .args(["mounts", "scan", "--format", "json"])
        .output()
        .expect("scan mounts as JSON");
    assert_success(&concise_json);
    assert!(concise_json.stderr.is_empty());
    let concise_json: Value = serde_json::from_slice(&concise_json.stdout).expect("valid JSON");
    let skipped = google_skip(&concise_json);
    assert!(skipped.get("candidates").is_none());

    let verbose_json = fixture
        .command()
        .args(["-v", "mounts", "scan", "-f", "json"])
        .output()
        .expect("scan mounts as verbose JSON");
    assert_success(&verbose_json);
    assert!(verbose_json.stderr.is_empty());
    let verbose_json: Value = serde_json::from_slice(&verbose_json.stdout).expect("valid JSON");
    let candidates = google_skip(&verbose_json)["candidates"]
        .as_array()
        .expect("verbose candidate list");
    assert_eq!(candidates.len(), 2);
    for candidate in &fixture.google {
        assert!(
            candidates
                .iter()
                .any(|value| value == candidate.to_str().unwrap())
        );
    }
}

fn assert_destination(output: &Output, destination: &Path) {
    assert_success(output);
    assert_eq!(
        output.stdout,
        format!("{}\n", destination.display()).as_bytes()
    );
    assert!(output.stderr.is_empty());
}

fn assert_concise_google_warning(stderr: &str, fixture: &Fixture) {
    assert!(stderr.contains("Skipped:"));
    assert!(stderr.contains("google-drive"));
    assert!(stderr.contains("2 possible paths"));
    assert!(stderr.contains("rerun with --verbose"));
    for candidate in &fixture.google {
        assert!(!stderr.contains(candidate.to_str().unwrap()));
    }
}

fn google_skip(report: &Value) -> &Value {
    report["skipped"]
        .as_array()
        .expect("skipped JSON array")
        .iter()
        .find(|item| item["name"] == "google-drive")
        .expect("Google Drive skip")
}

fn path_message(message: &str, path: &Path) -> Vec<u8> {
    format!("{message} {}\n", path.display()).into_bytes()
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}
