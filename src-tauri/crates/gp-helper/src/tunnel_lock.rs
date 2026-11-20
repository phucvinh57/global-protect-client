//! One lock for all GUI/CLI users, automatically released on process exit.
use std::{
    fs::{File, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
};

pub fn acquire() -> Result<File, String> {
    acquire_at(std::path::Path::new("/run/lock/gp-client-tunnel.lock"), 0)
}

fn acquire_at(path: &std::path::Path, owner: u32) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| format!("Cannot open system tunnel lock: {e}"))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.uid() != owner || meta.nlink() != 1 || meta.mode() & 0o022 != 0 {
        return Err("System tunnel lock must be a root-owned regular file".into());
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o644))
        .map_err(|e| e.to_string())?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = io::Error::last_os_error();
        return Err(if error.kind() == io::ErrorKind::WouldBlock {
            "Another instance owns the VPN tunnel (GUI or CLI)".into()
        } else {
            format!("Cannot lock VPN tunnel: {error}")
        });
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn competing_gui_cli_owners_wait_until_cleanup_releases_lock() {
        let directory =
            std::env::temp_dir().join(format!("gp-helper-lock-test-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("tunnel.lock");
        let owner = unsafe { libc::geteuid() };
        let first = acquire_at(&path, owner).unwrap();
        assert!(acquire_at(&path, owner)
            .unwrap_err()
            .contains("Another instance"));
        drop(first);
        let second = acquire_at(&path, owner).unwrap();
        drop(second);
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(directory.join("target"), &path).unwrap();
        assert!(acquire_at(&path, owner).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
