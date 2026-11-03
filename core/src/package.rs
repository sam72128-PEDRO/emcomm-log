//! Portable `.emlog` packages: a ZIP with `manifest.json`, `logbook.sqlite`, and
//! `attachments/<sha256>.<ext>`. Export never mutates the live logbook; import always creates a
//! new local logbook identity and never overwrites or merges.

use crate::backup::{InstallOutcome, finalize_install};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{self, short_uuid};
use crate::logbook::{self, ATTACHMENTS_DIR, DB_FILE, is_attachment_name};
use crate::migrate::{self, LOGBOOK_MIGRATIONS};
use crate::snapshot::{self, FORMAT_VERSION, MANIFEST_FILE, Manifest, PACKAGE_FORMAT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

pub const MAX_PACKAGE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_DB_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 200_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPackageResult {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub entry_count: i64,
    pub attachment_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackagePreview {
    pub source_uuid: String,
    pub name: String,
    pub logbook_type: crate::models::LogbookType,
    pub created_at_utc: String,
    pub app_version: String,
    pub entry_count: i64,
    pub attachment_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub path: String,
    pub logbook_id: String,
    pub source_uuid: String,
    pub entry_count: i64,
    pub duplicate_of: Vec<String>,
}

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InvalidPackage, msg)
}

pub fn export_package(live_dir: &Path, dest_file: &Path) -> AppResult<ExportPackageResult> {
    let parent = dest_file.parent().ok_or_else(|| AppError::validation("path", "Choose a file location"))?;
    if !parent.is_dir() {
        return Err(AppError::new(ErrorCode::StorageUnavailable, "Export folder is not available"));
    }
    if fsutil::is_within(parent, live_dir) {
        return Err(AppError::validation("path", "Save the package outside the live logbook folder"));
    }
    let work = parent.join(format!(".emcomm-export-{}", short_uuid(&fsutil::new_uuid())));
    let tmp_zip = parent.join(format!(
        ".{}.tmp-{}",
        dest_file.file_name().and_then(|n| n.to_str()).unwrap_or("export.emlog"),
        short_uuid(&fsutil::new_uuid())
    ));
    let result = (|| -> AppResult<ExportPackageResult> {
        fs::create_dir(&work)?;
        let snap = snapshot::take_snapshot(live_dir, &work)?;
        let manifest = snapshot::manifest_for(&snap, PACKAGE_FORMAT, None);
        {
            let f = OpenOptions::new().write(true).create_new(true).open(&tmp_zip)?;
            let mut zw = zip::ZipWriter::new(BufWriter::new(f));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .large_file(true)
                .unix_permissions(0o644);
            zw.start_file(MANIFEST_FILE, opts).map_err(zip_err)?;
            zw.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
            for mf in &manifest.files {
                zw.start_file(mf.path.as_str(), opts).map_err(zip_err)?;
                let mut src = BufReader::new(File::open(work.join(&mf.path))?);
                std::io::copy(&mut src, &mut zw)?;
            }
            crate::faults::check("package.write")?;
            let w = zw.finish().map_err(zip_err)?;
            let f = w.into_inner().map_err(|e| AppError::from(e.into_error()))?;
            f.sync_all()?;
        }
        verify_package_file(&tmp_zip).map_err(|e| e.with_context("Exported package failed verification"))?;
        fs::rename(&tmp_zip, dest_file)?;
        fsutil::sync_dir(parent)?;
        let (sha, bytes) = fsutil::sha256_file(dest_file)?;
        Ok(ExportPackageResult {
            path: dest_file.to_string_lossy().to_string(),
            bytes,
            sha256: sha,
            entry_count: snap.entry_count,
            attachment_count: snap.files.len() - 1,
        })
    })();
    let _ = fs::remove_dir_all(&work);
    if result.is_err() {
        let _ = fs::remove_file(&tmp_zip);
    }
    result
}

fn zip_err(e: zip::result::ZipError) -> AppError {
    match e {
        zip::result::ZipError::Io(io) => AppError::from(io),
        other => invalid(format!("Archive error: {other}")),
    }
}

struct CheckedArchive {
    manifest: Manifest,
    names: Vec<(usize, String)>,
}

fn is_symlink_mode(mode: Option<u32>) -> bool {
    mode.map(|m| (m & 0o170000) == 0o120000).unwrap_or(false)
}

/// Structural validation of the archive and manifest, without extracting anything.
fn check_archive(archive: &mut zip::ZipArchive<BufReader<File>>) -> AppResult<CheckedArchive> {
    if archive.len() > MAX_ENTRIES {
        return Err(invalid("Package contains too many files"));
    }
    let mut seen = HashSet::new();
    let mut seen_ci = HashSet::new();
    let mut names = Vec::new();
    let mut total: u64 = 0;
    let mut manifest_idx = None;
    for i in 0..archive.len() {
        let f = archive.by_index_raw(i).map_err(zip_err)?;
        let name = std::str::from_utf8(f.name_raw()).map_err(|_| invalid("Package contains a non-UTF-8 file name"))?.to_string();
        if f.encrypted() {
            return Err(invalid("Encrypted packages are not supported"));
        }
        if is_symlink_mode(f.unix_mode()) {
            return Err(invalid(format!("Package contains a symbolic link: {name}")));
        }
        if f.is_dir() {
            if name == "attachments/" {
                continue;
            }
            return Err(invalid(format!("Unexpected folder in package: {name}")));
        }
        if !fsutil::is_plain_relative(&name) {
            return Err(invalid(format!("Unsafe path in package: {name}")));
        }
        let allowed = name == MANIFEST_FILE
            || name == DB_FILE
            || name.strip_prefix("attachments/").map(|rest| !rest.contains('/') && is_attachment_name(rest)).unwrap_or(false);
        if !allowed {
            return Err(invalid(format!("Unexpected file in package: {name}")));
        }
        if !seen.insert(name.clone()) || !seen_ci.insert(name.to_lowercase()) {
            return Err(invalid(format!("Duplicate file name in package: {name}")));
        }
        let limit = if name == MANIFEST_FILE {
            MAX_MANIFEST_BYTES
        } else if name == DB_FILE {
            MAX_DB_BYTES
        } else {
            logbook::MAX_ATTACHMENT_BYTES
        };
        if f.size() > limit {
            return Err(invalid(format!("File too large in package: {name}")));
        }
        total = total.saturating_add(f.size());
        if total > MAX_TOTAL_UNCOMPRESSED {
            return Err(invalid("Package expands beyond the supported size"));
        }
        if name == MANIFEST_FILE {
            manifest_idx = Some(i);
        }
        names.push((i, name));
    }
    let mi = manifest_idx.ok_or_else(|| invalid("Package has no manifest.json"))?;
    let mut raw = Vec::new();
    archive.by_index(mi).map_err(zip_err)?.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut raw)?;
    let manifest: Manifest = serde_json::from_slice(&raw).map_err(|e| invalid(format!("manifest.json is malformed: {e}")))?;
    if manifest.format != PACKAGE_FORMAT {
        return Err(invalid("This file is not an EMCOMM LOG package"));
    }
    if manifest.format_version > FORMAT_VERSION {
        return Err(AppError::new(
            ErrorCode::UnsupportedVersion,
            format!("Package format version {} is newer than this app supports ({FORMAT_VERSION})", manifest.format_version),
        ));
    }
    if manifest.schema_version > migrate::latest_version(LOGBOOK_MIGRATIONS) {
        return Err(AppError::new(ErrorCode::UnsupportedVersion, "Package was created by a newer version of EMCOMM LOG"));
    }
    if uuid::Uuid::parse_str(&manifest.source.logbook_id).is_err() {
        return Err(invalid("Manifest source logbook id is invalid"));
    }
    let listed: HashSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    if listed.len() != manifest.files.len() {
        return Err(invalid("Manifest lists a file twice"));
    }
    let present: HashSet<&str> = names.iter().map(|(_, n)| n.as_str()).filter(|n| *n != MANIFEST_FILE).collect();
    if listed != present {
        return Err(invalid("Package contents do not match the manifest file list"));
    }
    if !listed.contains(DB_FILE) {
        return Err(invalid("Package has no logbook database"));
    }
    for f in &manifest.files {
        if f.sha256.len() != 64 || !f.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid(format!("Manifest hash for {} is malformed", f.path)));
        }
    }
    Ok(CheckedArchive { manifest, names })
}

/// Number of records declared by the ZIP end-of-central-directory (zip64 aware). The zip crate
/// indexes members by name, so duplicates collapse; comparing counts exposes them.
fn central_directory_count(path: &Path) -> AppResult<u64> {
    use std::io::{Seek, SeekFrom};
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let tail_len = len.min(65_535 + 22);
    f.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail)?;
    let pos = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(|| invalid("The file is not a valid .emlog archive"))?;
    let count16 = u16::from_le_bytes([tail[pos + 10], tail[pos + 11]]) as u64;
    if count16 != 0xFFFF {
        return Ok(count16);
    }
    if pos < 20 || tail[pos - 20..pos - 16] != [0x50, 0x4b, 0x06, 0x07] {
        return Err(invalid("Malformed zip64 archive"));
    }
    let off = u64::from_le_bytes(tail[pos - 12..pos - 4].try_into().unwrap());
    f.seek(SeekFrom::Start(off))?;
    let mut rec = [0u8; 56];
    f.read_exact(&mut rec)?;
    if rec[0..4] != [0x50, 0x4b, 0x06, 0x06] {
        return Err(invalid("Malformed zip64 archive"));
    }
    Ok(u64::from_le_bytes(rec[32..40].try_into().unwrap()))
}

fn open_archive(path: &Path) -> AppResult<zip::ZipArchive<BufReader<File>>> {
    let meta = fs::metadata(path).map_err(|e| AppError::from(e).with_context("Cannot read package"))?;
    if !meta.is_file() {
        return Err(invalid("Not a file"));
    }
    if meta.len() > MAX_PACKAGE_BYTES {
        return Err(invalid("Package file is larger than the supported maximum (8 GB)"));
    }
    let declared = central_directory_count(path)?;
    let archive = zip::ZipArchive::new(BufReader::new(File::open(path)?)).map_err(|_| invalid("The file is not a valid .emlog archive"))?;
    if declared != archive.len() as u64 {
        return Err(invalid("Package contains duplicate file names"));
    }
    Ok(archive)
}

/// Stream every member and verify size and SHA-256 against the manifest.
pub fn verify_package_file(path: &Path) -> AppResult<Manifest> {
    let mut archive = open_archive(path)?;
    let checked = check_archive(&mut archive)?;
    let expected: HashMap<&str, &snapshot::ManifestFile> = checked.manifest.files.iter().map(|f| (f.path.as_str(), f)).collect();
    for (i, name) in &checked.names {
        if name == MANIFEST_FILE {
            continue;
        }
        let mf = expected[name.as_str()];
        let mut zf = archive.by_index(*i).map_err(zip_err)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        let mut total = 0u64;
        loop {
            let n = zf.read(&mut buf).map_err(|e| invalid(format!("Cannot read {name}: {e}")))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > mf.bytes {
                return Err(invalid(format!("{name} is larger than declared")));
            }
            h.update(&buf[..n]);
        }
        if total != mf.bytes || hex::encode(h.finalize()) != mf.sha256 {
            return Err(AppError::new(ErrorCode::Integrity, format!("{name} does not match its manifest hash")));
        }
    }
    Ok(checked.manifest)
}

pub fn preview_package(path: &Path) -> AppResult<PackagePreview> {
    let m = verify_package_file(path)?;
    Ok(PackagePreview {
        source_uuid: m.source.logbook_id,
        name: m.source.name,
        logbook_type: m.source.logbook_type,
        created_at_utc: m.created_at_utc,
        app_version: m.app_version,
        entry_count: m.entry_count,
        attachment_count: m.files.len().saturating_sub(1),
    })
}

/// Import a package as a brand-new local logbook under `parent`.
/// `known` maps local logbook ids and their recorded source UUIDs to names, for duplicate reporting.
pub fn import_package(
    package: &Path,
    parent: &Path,
    name_override: Option<&str>,
    known: &[(String, Option<String>, String)],
) -> AppResult<ImportResult> {
    fsutil::check_writable(parent)?;
    logbook::check_live_location(parent)?;
    let mut archive = open_archive(package)?;
    let checked = check_archive(&mut archive)?;
    let expected: HashMap<String, snapshot::ManifestFile> = checked.manifest.files.iter().map(|f| (f.path.clone(), f.clone())).collect();
    let tmp = parent.join(format!(".emcomm-importing-{}", short_uuid(&fsutil::new_uuid())));
    let extract = (|| -> AppResult<()> {
        fs::create_dir(&tmp)?;
        fs::create_dir(tmp.join(ATTACHMENTS_DIR))?;
        for (i, name) in &checked.names {
            if name == MANIFEST_FILE {
                continue;
            }
            let mf = &expected[name];
            let dest = tmp.join(name);
            if !fsutil::is_within(dest.parent().unwrap(), &tmp) {
                return Err(invalid("Unsafe path in package"));
            }
            let mut zf = archive.by_index(*i).map_err(zip_err)?;
            let mut out = OpenOptions::new().write(true).create_new(true).open(&dest)?;
            let mut h = Sha256::new();
            let mut buf = vec![0u8; 256 * 1024];
            let mut total = 0u64;
            loop {
                let n = zf.read(&mut buf).map_err(|e| invalid(format!("Cannot read {name}: {e}")))?;
                if n == 0 {
                    break;
                }
                total += n as u64;
                if total > mf.bytes {
                    return Err(invalid(format!("{name} is larger than declared")));
                }
                h.update(&buf[..n]);
                out.write_all(&buf[..n])?;
            }
            out.sync_all()?;
            if total != mf.bytes || hex::encode(h.finalize()) != mf.sha256 {
                return Err(AppError::new(ErrorCode::Integrity, format!("{name} does not match its manifest hash")));
            }
        }
        {
            let conn = rusqlite::Connection::open(tmp.join(DB_FILE)).map_err(|_| invalid("Package database cannot be opened"))?;
            logbook::integrity_check(&conn).map_err(|e| AppError::new(ErrorCode::InvalidPackage, e.message))?;
            let has_logbook: i64 = conn
                .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='logbook'", [], |r| r.get(0))
                .map_err(|_| invalid("Package database is not a logbook"))?;
            if has_logbook != 1 {
                return Err(invalid("Package database is not an EMCOMM LOG logbook"));
            }
            let id: String = conn
                .query_row("SELECT id FROM logbook", [], |r| r.get(0))
                .map_err(|_| invalid("Package database has no logbook identity"))?;
            if id != checked.manifest.source.logbook_id {
                return Err(invalid("Package database identity does not match its manifest"));
            }
        }
        fsutil::sync_dir(&tmp)?;
        Ok(())
    })();
    if let Err(e) = extract {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let src = checked.manifest.source.logbook_id.clone();
    let duplicate_of: Vec<String> = known
        .iter()
        .filter(|(id, source, _)| *id == src || source.as_deref() == Some(src.as_str()))
        .map(|(_, _, name)| name.clone())
        .collect();
    let out: InstallOutcome = finalize_install(
        &tmp,
        parent,
        name_override,
        true,
        serde_json::json!({"action": "import", "package_created_at_utc": checked.manifest.created_at_utc, "package_app_version": checked.manifest.app_version}),
    )?;
    Ok(ImportResult {
        path: out.path.to_string_lossy().to_string(),
        logbook_id: out.logbook_id,
        source_uuid: out.source_uuid,
        entry_count: out.entry_count,
        duplicate_of,
    })
}
