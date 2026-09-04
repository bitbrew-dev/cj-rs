use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{MountProvider, MountSpec};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Platform {
    Macos,
    Other,
}

#[derive(Debug)]
pub struct DiscoveryContext {
    platform: Platform,
    home: PathBuf,
    volumes_root: PathBuf,
    cloud_storage_root: PathBuf,
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
        } else {
            Platform::Other
        };
        let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        if platform == Platform::Macos && home.as_os_str().is_empty() {
            return Err("HOME is not set".into());
        }
        Ok(Self::new(
            platform,
            home.clone(),
            "/Volumes".into(),
            home.join("Library/CloudStorage"),
        ))
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

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Volume,
    Icloud,
    GoogleDrive,
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
    if context.platform != Platform::Macos {
        report
            .warnings
            .push("automatic mount discovery is supported only on macOS".into());
        return report;
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
    context: &DiscoveryContext,
) -> Result<PathBuf, String> {
    if context.platform != Platform::Macos {
        return Err("automatic mount providers are supported only on macOS".into());
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
    };
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
    path.is_dir().then(|| fs::canonicalize(path).ok()).flatten()
}

fn slug(name: &str) -> Option<String> {
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
        },
    )]);
    toml::to_string(&Snippet { mounts }).expect("mount snippet serialization")
}
