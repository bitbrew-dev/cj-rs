use std::env;
use std::path::{Path, PathBuf};

use windows::Storage::IStorageItem;
use windows::Storage::Provider::StorageProviderSyncRootManager;
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE};
use windows::core::{HSTRING, Interface};

use crate::mounts::{Source, WindowsMount};

pub fn collect() -> (Vec<WindowsMount>, Vec<String>) {
    let mut mounts = Vec::new();
    let mut warnings = Vec::new();
    collect_sync_roots(&mut mounts, &mut warnings);
    collect_drives(&mut mounts, &mut warnings);
    (mounts, warnings)
}

fn collect_sync_roots(mounts: &mut Vec<WindowsMount>, warnings: &mut Vec<String>) {
    // SAFETY: this initializes the current CLI thread and is balanced below.
    let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    if let Err(error) = initialized {
        warnings.push(format!(
            "cannot initialize Windows cloud discovery: {error}"
        ));
        return;
    }
    let result = (|| -> windows::core::Result<()> {
        let roots = StorageProviderSyncRootManager::GetCurrentSyncRoots()?;
        for index in 0..roots.Size()? {
            let root = roots.GetAt(index)?;
            let id = root.Id()?.to_string();
            let folder = root.Path()?;
            let path = folder.cast::<IStorageItem>()?.Path()?.to_string();
            let path = PathBuf::from(path);
            if !path.is_dir() {
                continue;
            }
            let (name, source) = classify_sync_root(&id, &path);
            mounts.push(WindowsMount::ready(name, source, path));
        }
        Ok(())
    })();
    // SAFETY: RoInitialize succeeded on this thread.
    unsafe { RoUninitialize() };
    if let Err(error) = result {
        warnings.push(format!("cannot enumerate Windows cloud roots: {error}"));
    }
}

fn classify_sync_root(id: &str, path: &Path) -> (String, Source) {
    let mut fields = id.splitn(3, '!');
    let provider = fields.next().unwrap_or_default();
    let _sid = fields.next();
    let account = fields.next().unwrap_or_default();
    if provider.eq_ignore_ascii_case("onedrive") {
        if account.eq_ignore_ascii_case("personal") {
            return ("onedrive-personal".into(), Source::OneDrivePersonal);
        }
        if account.to_ascii_lowercase().starts_with("business") {
            return ("onedrive-business".into(), Source::OneDriveBusiness);
        }
        return ("onedrive".into(), Source::OneDrive);
    }
    let fallback = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(provider);
    (
        super::mounts::slug(fallback).unwrap_or_else(|| "cloud-drive".into()),
        Source::Cloud,
    )
}

fn collect_drives(mounts: &mut Vec<WindowsMount>, warnings: &mut Vec<String>) {
    // SAFETY: GetLogicalDrives has no arguments and returns a bitmask.
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        warnings.push("cannot enumerate Windows drive roots".into());
        return;
    }
    let system_drive = env::var("SystemDrive").ok();
    for offset in 0..26 {
        if mask & (1 << offset) == 0 {
            continue;
        }
        let letter = char::from(b'A' + offset as u8);
        let root = format!("{letter}:\\");
        if system_drive
            .as_deref()
            .is_some_and(|system| root[..2].eq_ignore_ascii_case(system))
        {
            continue;
        }
        let root_string = HSTRING::from(&root);
        // SAFETY: root_string is a valid, NUL-terminated Windows string.
        let kind = unsafe { GetDriveTypeW(&root_string) };
        let path = PathBuf::from(&root);
        if !path.is_dir() || !matches!(kind, DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_REMOTE) {
            continue;
        }
        let label = (kind == DRIVE_FIXED)
            .then(|| volume_label(&root_string))
            .flatten();
        let name = label
            .as_deref()
            .and_then(super::mounts::slug)
            .unwrap_or_else(|| format!("drive-{}", letter.to_ascii_lowercase()));
        let dynamic = kind == DRIVE_REMOTE
            || label.as_deref().is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("google drive") || value.contains("cloud")
            });
        mounts.push(if dynamic {
            WindowsMount::dynamic(name, path)
        } else {
            WindowsMount::ready(name, Source::WindowsDrive, path)
        });
    }
}

fn volume_label(root: &HSTRING) -> Option<String> {
    let mut label = [0_u16; 261];
    // SAFETY: the mutable slice is a valid output buffer for the duration of the call.
    unsafe { GetVolumeInformationW(root, Some(&mut label), None, None, None, None) }
        .ok()
        .and_then(|()| {
            let length = label
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(label.len());
            let value = String::from_utf16_lossy(&label[..length]);
            (!value.is_empty()).then_some(value)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_only_documented_onedrive_id_parts() {
        assert_eq!(
            classify_sync_root(
                "OneDrive!S-1-2-3!Personal",
                Path::new(r"C:\Users\me\OneDrive")
            ),
            ("onedrive-personal".into(), Source::OneDrivePersonal)
        );
        assert_eq!(
            classify_sync_root(
                "OneDrive!S-1-2-3!Business2",
                Path::new(r"C:\Users\me\OneDrive - Acme"),
            ),
            ("onedrive-business".into(), Source::OneDriveBusiness)
        );
        assert_eq!(
            classify_sync_root("OneDrive!opaque", Path::new(r"C:\Users\me\Cloud")),
            ("onedrive".into(), Source::OneDrive)
        );
    }
}
