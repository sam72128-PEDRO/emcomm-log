use crate::error::{AppError, AppResult, ErrorCode};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn short_uuid(id: &str) -> String {
    id.chars().filter(|c| *c != '-').take(8).collect()
}

/// fsync a directory so a rename/create inside it is durable. No-op where unsupported.
pub fn sync_dir(dir: &Path) -> AppResult<()> {
    #[cfg(unix)]
    {
        let f = File::open(dir)?;
        f.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}

/// Write bytes to `path` atomically: temp file in the same directory, fsync, rename, fsync dir.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let dir = path.parent().ok_or_else(|| AppError::internal("path has no parent"))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("file"),
        short_uuid(&new_uuid())
    ));
    let result = (|| -> AppResult<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        sync_dir(dir)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Prove a directory is writable by creating, syncing, and removing a probe file.
pub fn check_writable(dir: &Path) -> AppResult<()> {
    if !dir.exists() {
        return Err(AppError::new(ErrorCode::StorageUnavailable, format!("Folder does not exist: {}", dir.display())));
    }
    if !dir.is_dir() {
        return Err(AppError::new(ErrorCode::Validation, "The selected path is not a folder"));
    }
    let probe = dir.join(format!(".emcomm-write-test-{}", short_uuid(&new_uuid())));
    let r = (|| -> AppResult<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&probe)?;
        f.write_all(b"emcomm-log write probe")?;
        f.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(&probe);
    r.map_err(|e| e.with_context("Folder is not writable"))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Streaming SHA-256 of a file; returns (hex, size).
pub fn sha256_file(path: &Path) -> AppResult<(String, u64)> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        total += n as u64;
    }
    Ok((hex::encode(h.finalize()), total))
}

/// Copy a file and fsync the destination. Returns (sha256, size) of the bytes written.
pub fn copy_synced(src: &Path, dst: &Path) -> AppResult<(String, u64)> {
    let mut input = File::open(src)?;
    let mut out = OpenOptions::new().write(true).create_new(true).open(dst)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        h.update(&buf[..n]);
        total += n as u64;
    }
    out.sync_all()?;
    Ok((hex::encode(h.finalize()), total))
}

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make a name safe as a single path component on Windows, macOS, and Linux.
/// Unicode letters are preserved; separators, reserved characters and controls are replaced.
pub fn safe_component(name: &str, max_chars: usize) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c if c.is_whitespace() => '_',
            c => c,
        })
        .collect();
    out = out.trim_matches(|c| c == '.' || c == '_' || c == ' ').to_string();
    let mut truncated: String = out.chars().take(max_chars).collect();
    truncated = truncated.trim_end_matches(['.', ' ']).to_string();
    if truncated.is_empty() {
        truncated = "logbook".to_string();
    }
    let upper = truncated.split('.').next().unwrap_or("").to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&upper.as_str()) {
        truncated = format!("_{truncated}");
    }
    truncated
}

/// Lowercase, alphanumeric-only extension of at most 10 characters, or empty.
pub fn safe_extension(name: &str) -> String {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("");
    let ext: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    if ext.len() > 10 {
        String::new()
    } else {
        ext
    }
}

pub fn canonical_or_self(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// True if `child` equals or is inside `parent` (after canonicalization where possible).
pub fn is_within(child: &Path, parent: &Path) -> bool {
    let c = canonical_or_self(child);
    let p = canonical_or_self(parent);
    c.starts_with(&p)
}

/// A relative path with only normal components (no root, prefix, `..`, or `.`).
pub fn is_plain_relative(p: &str) -> bool {
    if p.is_empty() || p.contains('\\') || p.contains('\0') {
        return false;
    }
    let path = Path::new(p);
    path.components().all(|c| matches!(c, Component::Normal(_)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationKind {
    Local,
    Network,
    CloudSync,
}

/// Best-effort detection of network file systems and cloud-synchronized folders.
pub fn classify_location(path: &Path) -> LocationKind {
    let canon = canonical_or_self(path);
    let s = canon.to_string_lossy().to_string();
    let lower = s.to_lowercase();
    let cloud_markers = [
        "/library/cloudstorage/",
        "/library/mobile documents/",
        "/dropbox/",
        "/onedrive",
        "/google drive/",
        "/googledrive/",
        "/my drive/",
        "/icloud drive/",
        "/box sync/",
        "\\dropbox\\",
        "\\onedrive",
        "\\google drive\\",
        "\\icloudrive\\",
        "\\icloud drive\\",
        "\\box\\",
    ];
    let probe = format!("{}/", lower.trim_end_matches(['/', '\\']));
    if cloud_markers.iter().any(|m| probe.contains(m)) {
        return LocationKind::CloudSync;
    }
    if is_network_fs(&canon) {
        return LocationKind::Network;
    }
    LocationKind::Local
}

#[cfg(target_os = "macos")]
fn is_network_fs(path: &Path) -> bool {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else { return false };
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return false;
    }
    let name = unsafe { CStr::from_ptr(st.f_fstypename.as_ptr()) }.to_string_lossy().to_lowercase();
    let local_flag = (st.f_flags & libc::MNT_LOCAL as u32) != 0;
    matches!(name.as_str(), "smbfs" | "nfs" | "afpfs" | "webdav" | "cifs" | "ftp" | "osxfuse" | "macfuse") || !local_flag
}

#[cfg(target_os = "linux")]
fn is_network_fs(path: &Path) -> bool {
    let Ok(mounts) = fs::read_to_string("/proc/mounts") else { return false };
    let target = path.to_string_lossy().to_string();
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let mut parts = line.split_whitespace();
        let (_dev, mnt, fstype) = match (parts.next(), parts.next(), parts.next()) {
            (Some(a), Some(b), Some(c)) => (a, b.replace("\\040", " "), c.to_string()),
            _ => continue,
        };
        let matches = target == mnt || target.starts_with(&format!("{}/", mnt.trim_end_matches('/'))) || mnt == "/";
        if matches && best.as_ref().map(|(l, _)| mnt.len() > *l).unwrap_or(true) {
            best = Some((mnt.len(), fstype));
        }
    }
    match best {
        Some((_, t)) => {
            let t = t.to_lowercase();
            t.starts_with("nfs")
                || t == "cifs"
                || t.starts_with("smb")
                || t == "fuse.sshfs"
                || t == "fuse.rclone"
                || t == "9p"
                || t == "afs"
                || t == "ceph"
                || t == "glusterfs"
                || t == "davfs"
                || t == "fuse.gvfsd-fuse"
        }
        None => false,
    }
}

#[cfg(windows)]
fn is_network_fs(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    let s = path.to_string_lossy();
    if s.starts_with("\\\\") && !s.starts_with("\\\\?\\") {
        return true;
    }
    if s.starts_with("\\\\?\\UNC\\") {
        return true;
    }
    let root: Vec<u16> = {
        let comps: Vec<_> = path.components().take(2).collect();
        let mut root = PathBuf::new();
        for c in comps {
            root.push(c.as_os_str());
        }
        root.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    };
    let t = unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr()) };
    t == windows_sys::Win32::Storage::FileSystem::DRIVE_REMOTE
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn is_network_fs(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_component_handles_reserved_and_unicode() {
        assert_eq!(safe_component("Storm: Op/3", 60), "Storm__Op_3");
        assert_eq!(safe_component("CON", 60), "_CON");
        assert_eq!(safe_component("Ünïcödé 日本", 60), "Ünïcödé_日本");
        assert_eq!(safe_component("...", 60), "logbook");
        assert_eq!(safe_component("a".repeat(100).as_str(), 10).len(), 10);
    }

    #[test]
    fn plain_relative_rejects_traversal() {
        assert!(is_plain_relative("attachments/abc.txt"));
        assert!(!is_plain_relative("../x"));
        assert!(!is_plain_relative("/etc/passwd"));
        assert!(!is_plain_relative("a\\..\\b"));
        assert!(!is_plain_relative("attachments/../../x"));
        assert!(!is_plain_relative("./x"));
    }

    #[test]
    fn atomic_write_replaces_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f.json");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn cloud_paths_detected() {
        assert_eq!(
            classify_location(Path::new("/Users/x/Library/CloudStorage/Dropbox/logs")),
            LocationKind::CloudSync
        );
    }
}
