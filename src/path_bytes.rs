use std::borrow::Cow;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

pub fn from_bytes(bytes: &[u8], invalid_utf8: &str) -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        let _ = invalid_utf8;
        Ok(PathBuf::from(OsStr::from_bytes(bytes)))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes)
            .map(PathBuf::from)
            .map_err(|_| invalid_utf8.into())
    }
}

pub fn output_bytes(path: &Path) -> Result<Cow<'_, [u8]>, String> {
    #[cfg(unix)]
    {
        Ok(Cow::Borrowed(path.as_os_str().as_bytes()))
    }
    #[cfg(not(unix))]
    {
        path.to_str()
            .map(|path| Cow::Borrowed(path.as_bytes()))
            .ok_or("cannot write a non-Unicode path on this platform".into())
    }
}

pub fn without_line_ending(mut bytes: &[u8]) -> &[u8] {
    if let Some(stripped) = bytes.strip_suffix(b"\n") {
        bytes = stripped;
    }
    #[cfg(windows)]
    if let Some(stripped) = bytes.strip_suffix(b"\r") {
        bytes = stripped;
    }
    bytes
}
