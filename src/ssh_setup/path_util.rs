use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

/// Returns the current user's home directory, preferring `USERPROFILE` on
/// Windows and `HOME` elsewhere.
pub fn home_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    let key = "USERPROFILE";
    #[cfg(not(windows))]
    let key = "HOME";

    if let Some(val) = std::env::var_os(key)
        && !val.is_empty()
    {
        Ok(PathBuf::from(val))
    } else {
        bail!("could not determine home directory (`{key}` is not set)");
    }
}

#[cfg(unix)]
pub fn harden_dir_permissions(path: &Path) {
    set_mode(path, 0o700);
}

#[cfg(unix)]
pub fn harden_file_permissions(path: &Path) {
    set_mode(path, 0o600);
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    // Best-effort: the dir/file was already created successfully, so a
    // failure to tighten its permissions shouldn't abort the caller — just
    // warn, since ssh may later refuse a loosely-permissioned identity file.
    if let Err(err) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)) {
        eprintln!(
            "warning: failed to set permissions on {}: {err}",
            path.display()
        );
    }
}

#[cfg(not(unix))]
pub fn harden_dir_permissions(_path: &Path) {}

#[cfg(not(unix))]
pub fn harden_file_permissions(_path: &Path) {}
