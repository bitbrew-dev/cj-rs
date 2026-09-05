use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{MountProvider, MountSpec, OneDriveAccount};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Platform {
    Macos,
    Windows,
    Other,
}

#[derive(Debug)]
pub struct DiscoveryContext {
    platform: Platform,
    home: PathBuf,
    volumes_root: PathBuf,
    cloud_storage_root: PathBuf,
    windows_mounts: Vec<WindowsMount>,
    windows_warnings: Vec<String>,
}

impl DiscoveryContext {
    pub fn from_process() -> Result<Self, String> {
        if let Some(root) =
            env::var_os("CJ_INTERNAL_DISCOVERY_ROOT").filter(|root| !root.is_empty())
        {
            let root = PathBuf::from(root);
            let home = root.join("home");
            return Ok(Self::new(
                Platform::Macos,
                home.clone(),
                root.join("Volumes"),
                home.join("Library/CloudStorage"),
            ));
        }
        let platform = if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Other
        };
        let home = crate::config::home_dir().unwrap_or_default();
        if platform == Platform::Macos && home.as_os_str().is_empty() {
            return Err("HOME is not set".into());
        }
        let context = Self::new(
            platform,
            home.clone(),
            "/Volumes".into(),
            home.join("Library/CloudStorage"),
        );
        #[cfg(windows)]
        let context = if platform == Platform::Windows {
            let (windows_mounts, windows_warnings) = crate::windows_mounts::collect();
            Self {
                windows_mounts,
                windows_warnings,
                ..context
            }
        } else {
            context
        };
        Ok(context)
    }

    pub fn new(
        platform: Platform,
        home: PathBuf,
        volumes_root: PathBuf,
        cloud_storage_root: PathBuf,
    ) -> Self {
        Self {
            platform,
            home,
            volumes_root,
            cloud_storage_root,
            windows_mounts: Vec::new(),
            windows_warnings: Vec::new(),
        }
    }

    #[cfg(test)]
    fn windows(mounts: Vec<WindowsMount>, warnings: Vec<String>) -> Self {
        Self {
            platform: Platform::Windows,
            home: PathBuf::new(),
            volumes_root: PathBuf::new(),
            cloud_storage_root: PathBuf::new(),
            windows_mounts: mounts,
            windows_warnings: warnings,
        }
    }
}

#[derive(Debug)]
pub(crate) struct WindowsMount {
    name: String,
    source: Source,
    path: PathBuf,
    dynamic: bool,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl WindowsMount {
    pub(crate) fn ready(name: String, source: Source, path: PathBuf) -> Self {
        Self {
            name,
            source,
            path,
            dynamic: false,
        }
    }

    pub(crate) fn dynamic(name: String, path: PathBuf) -> Self {
        Self {
            name,
            source: Source::WindowsDrive,
            path,
            dynamic: true,
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ScanReport {
    pub ready: Vec<ReadyMount>,
    pub skipped: Vec<SkippedMount>,
    pub warnings: Vec<String>,
}

impl ScanReport {
    pub fn ready_mounts(&self) -> impl Iterator<Item = (String, PathBuf)> + '_ {
        self.ready
            .iter()
            .map(|mount| (mount.name.clone(), mount.path.clone()))
    }
}

#[derive(Debug, Serialize)]
pub struct ReadyMount {
    pub name: String,
    pub source: Source,
    pub path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct SkippedMount {
    pub name: String,
    pub source: Source,
    pub status: SkipStatus,
    pub reason: &'static str,
    pub candidates: Vec<PathBuf>,
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Volume,
    Icloud,
    GoogleDrive,
    Cloud,
    OneDrive,
    OneDrivePersonal,
    OneDriveBusiness,
    WindowsDrive,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkipStatus {
    Ambiguous,
    Skipped,
}

#[derive(Debug, PartialEq)]
pub struct RenderedScan {
    pub stdout: String,
    pub stderr: String,
}

pub fn scan(context: &DiscoveryContext) -> ScanReport {
    let mut report = ScanReport::default();
    match context.platform {
        Platform::Windows => {
            scan_windows(context, &mut report);
            return report;
        }
        Platform::Other => {
            report
                .warnings
                .push("automatic mount discovery is supported only on macOS and Windows".into());
            return report;
        }
        Platform::Macos => {}
    }

    let icloud = canonical_directory(
        &context
            .home
            .join("Library/Mobile Documents/com~apple~CloudDocs"),
    );
    let google = google_candidates(context, &mut report.warnings);
    let mut provider_paths = google.clone();
    if let Some(path) = icloud.as_ref() {
        provider_paths.insert(path.clone());
        report
            .ready
            .push(ready("icloud", Source::Icloud, path.clone()));
    }
    match google.len() {
        0 => {}
        1 => report.ready.push(ready(
            "google-drive",
            Source::GoogleDrive,
            google.into_iter().next().expect("one Google path"),
        )),
        _ => report.skipped.push(skipped(
            "google-drive",
            Source::GoogleDrive,
            SkipStatus::Ambiguous,
            "multiple-provider-paths",
            google.into_iter().collect(),
        )),
    }

    let mut volumes: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for path in matching_children(
        &context.volumes_root,
        |name| !name.starts_with('.'),
        &mut report.warnings,
    ) {
        if path == Path::new("/") || provider_paths.contains(&path) {
            continue;
        }
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(slug)
        else {
            report.skipped.push(skipped(
                "volume",
                Source::Volume,
                SkipStatus::Skipped,
                "invalid-volume-name",
                vec![path],
            ));
            continue;
        };
        volumes.entry(name).or_default().push(path);
    }
    for (name, paths) in volumes {
        if paths.len() == 1 && !report.ready.iter().any(|mount| mount.name == name) {
            report.ready.push(ready(
                name,
                Source::Volume,
                paths.into_iter().next().expect("one volume path"),
            ));
        } else {
            report.skipped.push(skipped(
                name,
                Source::Volume,
                SkipStatus::Ambiguous,
                "mount-name-collision",
                paths,
            ));
        }
    }
    report
        .ready
        .sort_by(|left, right| left.name.cmp(&right.name));
    report
        .skipped
        .sort_by(|left, right| left.name.cmp(&right.name));
    report
}

fn ready(name: impl Into<String>, source: Source, path: PathBuf) -> ReadyMount {
    ReadyMount {
        name: name.into(),
        source,
        path,
    }
}

fn skipped(
    name: impl Into<String>,
    source: Source,
    status: SkipStatus,
    reason: &'static str,
    candidates: Vec<PathBuf>,
) -> SkippedMount {
    SkippedMount {
        name: name.into(),
        source,
        status,
        reason,
        candidates,
    }
}

pub fn resolve_provider(
    provider: MountProvider,
    account: Option<OneDriveAccount>,
    context: &DiscoveryContext,
) -> Result<PathBuf, String> {
    if provider == MountProvider::OneDrive {
        if context.platform != Platform::Windows {
            return Err("onedrive provider is supported only on Windows".into());
        }
        let candidates = context
            .windows_mounts
            .iter()
            .filter(|mount| !mount.dynamic)
            .filter(|mount| match account {
                None => matches!(
                    mount.source,
                    Source::OneDrive | Source::OneDrivePersonal | Source::OneDriveBusiness
                ),
                Some(OneDriveAccount::Personal) => mount.source == Source::OneDrivePersonal,
                Some(OneDriveAccount::Business) => mount.source == Source::OneDriveBusiness,
            })
            .map(|mount| mount.path.clone())
            .collect::<Vec<_>>();
        return one_candidate("onedrive", candidates);
    }
    if context.platform != Platform::Macos {
        return Err("icloud and google-drive providers are supported only on macOS".into());
    }
    let (name, candidates) = match provider {
        MountProvider::Icloud => (
            "icloud",
            canonical_directory(
                &context
                    .home
                    .join("Library/Mobile Documents/com~apple~CloudDocs"),
            )
            .into_iter()
            .collect(),
        ),
        MountProvider::GoogleDrive => {
            let mut warnings = Vec::new();
            ("google-drive", google_candidates(context, &mut warnings))
        }
        MountProvider::OneDrive => unreachable!("handled above"),
    };
    one_candidate(name, candidates.into_iter().collect())
}

fn one_candidate(name: &str, candidates: Vec<PathBuf>) -> Result<PathBuf, String> {
    let candidates = deduplicate_windows_paths(candidates);
    match candidates.len() {
        1 => Ok(candidates.into_iter().next().expect("one provider path")),
        0 => Err(format!("{name} provider is not mounted")),
        count => Err(format!("{name} provider is ambiguous ({count} candidates)")),
    }
}

pub fn render_json(report: &ScanReport, verbose: bool) -> Result<RenderedScan, String> {
    #[derive(Serialize)]
    struct JsonReport<'a> {
        mounts: &'a [ReadyMount],
        skipped: Vec<JsonSkipped<'a>>,
        warnings: &'a [String],
    }
    #[derive(Serialize)]
    struct JsonSkipped<'a> {
        name: &'a str,
        source: Source,
        status: SkipStatus,
        reason: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        candidates: Option<&'a [PathBuf]>,
    }
    let output = JsonReport {
        mounts: &report.ready,
        skipped: report
            .skipped
            .iter()
            .map(|item| JsonSkipped {
                name: &item.name,
                source: item.source,
                status: item.status,
                reason: item.reason,
                candidates: verbose.then_some(item.candidates.as_slice()),
            })
            .collect(),
        warnings: &report.warnings,
    };
    serde_json::to_string_pretty(&output)
        .map(|stdout| RenderedScan {
            stdout,
            stderr: String::new(),
        })
        .map_err(|error| format!("cannot serialize mount scan: {error}"))
}

pub fn render_table(report: &ScanReport, verbose: bool) -> RenderedScan {
    let mut rows = vec![("NAME", "SOURCE", "STATUS", "PATH".into())];
    rows.extend(report.ready.iter().map(|mount| {
        (
            mount.name.as_str(),
            source_name(mount.source),
            "ready",
            mount.path.display().to_string(),
        )
    }));
    rows.extend(report.skipped.iter().map(|mount| {
        (
            mount.name.as_str(),
            source_name(mount.source),
            match mount.status {
                SkipStatus::Ambiguous => "ambiguous",
                SkipStatus::Skipped => "skipped",
            },
            format!("{} candidate(s)", mount.candidates.len()),
        )
    }));
    let widths = [0, 1, 2].map(|index| {
        rows.iter()
            .map(|row| [row.0, row.1, row.2][index].len())
            .max()
            .unwrap_or(0)
    });
    let stdout = rows
        .into_iter()
        .map(|row| {
            format!(
                "{:<w0$}  {:<w1$}  {:<w2$}  {}",
                row.0,
                row.1,
                row.2,
                row.3,
                w0 = widths[0],
                w1 = widths[1],
                w2 = widths[2]
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut diagnostics = Vec::new();
    if !report.skipped.is_empty() {
        diagnostics.push("Skipped:".into());
        for mount in &report.skipped {
            let detail = match mount.reason {
                "multiple-provider-paths" => {
                    format!("{} possible paths", mount.candidates.len())
                }
                "mount-name-collision" => format!(
                    "{} paths produce the same mount name",
                    mount.candidates.len()
                ),
                "invalid-volume-name" => "path cannot be converted to a mount name".into(),
                reason => reason.replace('-', " "),
            };
            diagnostics.push(format!(
                "  [warn] {}: {}{}",
                mount.name,
                detail,
                if verbose {
                    ""
                } else {
                    "; rerun with --verbose for candidate paths"
                }
            ));
            if verbose {
                for path in &mount.candidates {
                    diagnostics.push(format!("    candidate: {}", path.display()));
                    diagnostics.push(mount_toml(&mount.name, path).trim_end().into());
                }
            }
        }
    }
    diagnostics.extend(
        report
            .warnings
            .iter()
            .map(|warning| format!("cj: warning: {warning}")),
    );
    if verbose && !report.ready.is_empty() {
        diagnostics.push("Suggested config:".into());
        for ready in &report.ready {
            diagnostics.push(mount_toml(&ready.name, &ready.path).trim_end().into());
        }
    }
    RenderedScan {
        stdout,
        stderr: diagnostics.join("\n"),
    }
}

fn google_candidates(context: &DiscoveryContext, warnings: &mut Vec<String>) -> BTreeSet<PathBuf> {
    let mut paths = matching_children(
        &context.cloud_storage_root,
        |name| name.starts_with("GoogleDrive-"),
        warnings,
    );
    paths.extend(matching_children(
        &context.volumes_root,
        |name| name.starts_with("GoogleDrive"),
        warnings,
    ));
    paths
}

fn scan_windows(context: &DiscoveryContext, report: &mut ScanReport) {
    report
        .warnings
        .extend(context.windows_warnings.iter().cloned());
    let mut grouped: BTreeMap<String, Vec<&WindowsMount>> = BTreeMap::new();
    for mount in &context.windows_mounts {
        grouped.entry(mount.name.clone()).or_default().push(mount);
    }
    for (name, mounts) in grouped {
        let source = mounts[0].source;
        let paths =
            deduplicate_windows_paths(mounts.iter().map(|mount| mount.path.clone()).collect());
        if mounts.iter().any(|mount| mount.dynamic) {
            report.skipped.push(skipped(
                name,
                source,
                SkipStatus::Skipped,
                "dynamic-drive-letter",
                paths,
            ));
        } else if paths.len() == 1 {
            report.ready.push(ready(
                name,
                source,
                paths.into_iter().next().expect("one Windows path"),
            ));
        } else {
            let reason = if matches!(source, Source::OneDrive | Source::OneDriveBusiness) {
                "multiple-provider-paths"
            } else {
                "mount-name-collision"
            };
            report
                .skipped
                .push(skipped(name, source, SkipStatus::Ambiguous, reason, paths));
        }
    }
    report
        .ready
        .sort_by(|left, right| left.name.cmp(&right.name));
    report
        .skipped
        .sort_by(|left, right| left.name.cmp(&right.name));
}

fn deduplicate_windows_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique = BTreeMap::new();
    for path in paths {
        unique
            .entry(path.to_string_lossy().to_ascii_lowercase())
            .or_insert(path);
    }
    unique.into_values().collect()
}

fn matching_children(
    root: &Path,
    matches: impl Fn(&str) -> bool,
    warnings: &mut Vec<String>,
) -> BTreeSet<PathBuf> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return BTreeSet::new(),
        Err(error) => {
            warnings.push(format!("cannot scan {}: {error}", root.display()));
            return BTreeSet::new();
        }
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_str().is_some_and(&matches))
        .filter_map(|entry| canonical_directory(&entry.path()))
        .collect()
}

fn canonical_directory(path: &Path) -> Option<PathBuf> {
    path.is_dir()
        .then(|| dunce::canonicalize(path).ok())
        .flatten()
}

pub(crate) fn slug(name: &str) -> Option<String> {
    let value = name
        .chars()
        .fold((String::new(), false), |(mut output, dash), character| {
            if character.is_ascii_alphanumeric() {
                output.push(character.to_ascii_lowercase());
                (output, false)
            } else if !output.is_empty() && !dash {
                output.push('-');
                (output, true)
            } else {
                (output, dash)
            }
        })
        .0
        .trim_end_matches('-')
        .to_owned();
    (!value.is_empty()).then_some(value)
}

fn source_name(source: Source) -> &'static str {
    match source {
        Source::Volume => "volume",
        Source::Icloud => "icloud",
        Source::GoogleDrive => "google-drive",
        Source::Cloud => "cloud",
        Source::OneDrive => "onedrive",
        Source::OneDrivePersonal => "onedrive-personal",
        Source::OneDriveBusiness => "onedrive-business",
        Source::WindowsDrive => "windows-drive",
    }
}

fn mount_toml(name: &str, path: &Path) -> String {
    #[derive(Serialize)]
    struct Snippet {
        mounts: BTreeMap<String, MountSpec>,
    }
    let mounts = BTreeMap::from([(
        name.into(),
        MountSpec {
            path: Some(path.into()),
            provider: None,
            account: None,
        },
    )]);
    toml::to_string(&Snippet { mounts }).expect("mount snippet serialization")
}

#[cfg(test)]
#[path = "mounts_tests.rs"]
mod tests;
