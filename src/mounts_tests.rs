use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    volumes: PathBuf,
    cloud: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "cj-mounts-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let home = root.join("home");
        let volumes = root.join("Volumes");
        let cloud = home.join("Library/CloudStorage");
        fs::create_dir_all(&volumes).unwrap();
        fs::create_dir_all(&cloud).unwrap();
        Self {
            root,
            home,
            volumes,
            cloud,
        }
    }

    fn context(&self) -> DiscoveryContext {
        DiscoveryContext::new(
            Platform::Macos,
            self.home.clone(),
            self.volumes.clone(),
            self.cloud.clone(),
        )
    }

    fn mkdir(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        fs::create_dir_all(path).unwrap();
        path.into()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn discovers_and_deduplicates_provider_and_volume_paths() {
    let fixture = Fixture::new();
    let archive = fixture.mkdir(fixture.volumes.join("Archive Disk"));
    fixture.mkdir(
        fixture
            .home
            .join("Library/Mobile Documents/com~apple~CloudDocs"),
    );
    let google = fixture.mkdir(fixture.volumes.join("GoogleDriveLegacy"));
    fixture.mkdir(fixture.volumes.join(".timemachine"));
    #[cfg(unix)]
    std::os::unix::fs::symlink("/", fixture.volumes.join("Macintosh HD")).unwrap();

    let report = scan(&fixture.context());

    assert_eq!(
        report
            .ready_mounts()
            .collect::<BTreeMap<_, _>>()
            .get("archive-disk"),
        Some(&fs::canonicalize(archive).unwrap())
    );
    assert_eq!(
        resolve_provider(MountProvider::GoogleDrive, None, &fixture.context()).unwrap(),
        fs::canonicalize(google).unwrap()
    );
    assert_eq!(
        report.ready.len(),
        3,
        "provider, hidden, and root paths must not also be volumes"
    );
}

#[test]
fn skips_multiple_google_accounts_and_slug_collisions() {
    let fixture = Fixture::new();
    fixture.mkdir(fixture.cloud.join("GoogleDrive-a@example.com"));
    fixture.mkdir(fixture.cloud.join("GoogleDrive-b@example.com"));
    fixture.mkdir(fixture.volumes.join("Work SSD"));
    fixture.mkdir(fixture.volumes.join("work-ssd"));

    let report = scan(&fixture.context());

    assert!(report.ready.is_empty());
    assert_eq!(report.skipped.len(), 2);
    assert!(
        report
            .skipped
            .iter()
            .all(|item| matches!(item.status, SkipStatus::Ambiguous))
    );
    assert!(
        resolve_provider(MountProvider::GoogleDrive, None, &fixture.context())
            .unwrap_err()
            .contains("2 candidates")
    );
}

#[test]
fn renders_valid_json_and_verbose_parseable_toml() {
    let fixture = Fixture::new();
    let quoted = fixture.mkdir(fixture.volumes.join("Bob's Archive"));
    let report = scan(&fixture.context());

    let json = render_json(&report, false).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
    assert_eq!(
        value["mounts"][0]["path"],
        quoted.to_string_lossy().as_ref()
    );
    assert!(json.stderr.is_empty());

    let table = render_table(&report, true);
    assert!(!table.stdout.contains("[mounts."));
    let snippet = table.stderr.split("Suggested config:\n").nth(1).unwrap();
    let parsed: toml::Value = toml::from_str(snippet).unwrap();
    assert_eq!(
        parsed["mounts"]["bob-s-archive"]["path"].as_str(),
        quoted.to_str()
    );
}

#[test]
fn concise_json_hides_ambiguous_candidates_until_verbose() {
    let fixture = Fixture::new();
    fixture.mkdir(fixture.cloud.join("GoogleDrive-a@example.com"));
    fixture.mkdir(fixture.cloud.join("GoogleDrive-b@example.com"));
    let report = scan(&fixture.context());

    let concise: serde_json::Value =
        serde_json::from_str(&render_json(&report, false).unwrap().stdout).unwrap();
    let verbose: serde_json::Value =
        serde_json::from_str(&render_json(&report, true).unwrap().stdout).unwrap();
    assert!(concise["skipped"][0].get("candidates").is_none());
    assert_eq!(
        verbose["skipped"][0]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        render_table(&report, false)
            .stderr
            .starts_with("Skipped:\n")
    );
}

#[test]
fn other_platform_is_read_only_and_unsupported() {
    let fixture = Fixture::new();
    let context = DiscoveryContext::new(
        Platform::Other,
        fixture.home.clone(),
        fixture.volumes.clone(),
        fixture.cloud.clone(),
    );
    let report = scan(&context);
    assert!(report.ready.is_empty());
    assert!(report.warnings[0].contains("only on macOS"));
    assert!(resolve_provider(MountProvider::Icloud, None, &context).is_err());
}

#[test]
fn windows_keeps_unique_cloud_roots_and_skips_ambiguous_or_dynamic_mounts() {
    let fixture = Fixture::new();
    let personal = fixture.mkdir(fixture.root.join("OneDrive Personal"));
    let business_a = fixture.mkdir(fixture.root.join("OneDrive Work A"));
    let business_b = fixture.mkdir(fixture.root.join("OneDrive Work B"));
    let local = fixture.mkdir(fixture.root.join("Archive"));
    let dynamic = fixture.mkdir(fixture.root.join("Google Drive"));
    let context = DiscoveryContext::windows(
        vec![
            WindowsMount::ready(
                "onedrive-personal".into(),
                Source::OneDrivePersonal,
                personal.clone(),
            ),
            WindowsMount::ready(
                "onedrive-business".into(),
                Source::OneDriveBusiness,
                business_a.clone(),
            ),
            WindowsMount::ready(
                "onedrive-business".into(),
                Source::OneDriveBusiness,
                business_b.clone(),
            ),
            WindowsMount::ready("archive".into(), Source::WindowsDrive, local.clone()),
            WindowsMount::dynamic("google-drive".into(), dynamic.clone()),
            WindowsMount::ready("cloud-box".into(), Source::Cloud, fixture.root.clone()),
            WindowsMount::ready("onedrive".into(), Source::OneDrive, personal.clone()),
        ],
        vec!["cloud API partially unavailable".into()],
    );

    let report = scan(&context);
    let ready = report.ready_mounts().collect::<BTreeMap<_, _>>();
    assert_eq!(ready.get("onedrive-personal"), Some(&personal));
    assert_eq!(ready.get("archive"), Some(&local));
    assert!(report.skipped.iter().any(|mount| {
        mount.name == "onedrive-business" && matches!(mount.status, SkipStatus::Ambiguous)
    }));
    assert!(
        report.skipped.iter().any(|mount| {
            mount.name == "google-drive" && mount.reason == "dynamic-drive-letter"
        })
    );
    assert_eq!(report.warnings, ["cloud API partially unavailable"]);

    assert_eq!(
        resolve_provider(
            MountProvider::OneDrive,
            Some(OneDriveAccount::Personal),
            &context,
        ),
        Ok(personal)
    );
    assert!(
        resolve_provider(
            MountProvider::OneDrive,
            Some(OneDriveAccount::Business),
            &context,
        )
        .unwrap_err()
        .contains("2 candidates")
    );
}
