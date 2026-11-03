//! Unfinished-entry drafts, stored in the app's local recovery area (never the live database).
//! Each draft is one JSON file written atomically with a content checksum.

use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{self, now_utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DRAFT_SCHEMA_VERSION: u32 = 1;
const MAX_DRAFT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DraftMeta {
    pub draft_id: String,
    pub logbook_id: String,
    pub logbook_name: String,
    /// "new" for Quick Entry, "edit" for an existing entry
    pub editor: String,
    pub entry_id: Option<String>,
    pub entry_sequence: Option<i64>,
    pub base_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DraftFile {
    schema_version: u32,
    #[serde(flatten)]
    meta: DraftMeta,
    saved_at_utc: String,
    content: serde_json::Value,
    content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Draft {
    #[serde(flatten)]
    pub meta: DraftMeta,
    pub saved_at_utc: String,
    pub content: serde_json::Value,
    /// False when the checksum did not match: shown to the user but never auto-restored.
    pub intact: bool,
}

pub struct DraftStore {
    root: PathBuf,
}

fn check_id(field: &str, id: &str) -> AppResult<()> {
    uuid::Uuid::parse_str(id).map(|_| ()).map_err(|_| AppError::validation(field, "Invalid identifier"))
}

impl DraftStore {
    pub fn new(root: &Path) -> DraftStore {
        DraftStore { root: root.to_path_buf() }
    }

    fn dir_for(&self, logbook_id: &str) -> PathBuf {
        self.root.join(logbook_id)
    }

    pub fn save(&self, meta: &DraftMeta, content: &serde_json::Value) -> AppResult<String> {
        check_id("logbook_id", &meta.logbook_id)?;
        check_id("draft_id", &meta.draft_id)?;
        if !content.is_object() {
            return Err(AppError::validation("content", "Draft content must be an object"));
        }
        let content_str = content.to_string();
        if content_str.len() > MAX_DRAFT_BYTES {
            return Err(AppError::validation("content", "Draft is too large to store"));
        }
        crate::faults::check("draft.write")?;
        let saved_at = now_utc();
        let file = DraftFile {
            schema_version: DRAFT_SCHEMA_VERSION,
            meta: meta.clone(),
            saved_at_utc: saved_at.clone(),
            content: content.clone(),
            content_sha256: fsutil::sha256_bytes(content_str.as_bytes()),
        };
        let dir = self.dir_for(&meta.logbook_id);
        fs::create_dir_all(&dir).map_err(|e| AppError::from(e).with_context("Draft storage unavailable"))?;
        fsutil::atomic_write(&dir.join(format!("{}.json", meta.draft_id)), serde_json::to_string(&file)?.as_bytes())
            .map_err(|e| e.with_context("Draft save failed"))?;
        Ok(saved_at)
    }

    fn read_file(path: &Path) -> Option<Draft> {
        let bytes = fs::read(path).ok()?;
        let f: DraftFile = serde_json::from_slice(&bytes).ok()?;
        if f.schema_version > DRAFT_SCHEMA_VERSION {
            return None;
        }
        let intact = fsutil::sha256_bytes(f.content.to_string().as_bytes()) == f.content_sha256;
        Some(Draft { meta: f.meta, saved_at_utc: f.saved_at_utc, content: f.content, intact })
    }

    pub fn list(&self, logbook_id: &str) -> AppResult<Vec<Draft>> {
        check_id("logbook_id", logbook_id)?;
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir(self.dir_for(logbook_id)) else { return Ok(out) };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Some(d) = Self::read_file(&p)
                && d.meta.logbook_id == logbook_id
            {
                out.push(d);
            }
        }
        out.sort_by(|a, b| b.saved_at_utc.cmp(&a.saved_at_utc));
        Ok(out)
    }

    pub fn get(&self, logbook_id: &str, draft_id: &str) -> AppResult<Draft> {
        check_id("logbook_id", logbook_id)?;
        check_id("draft_id", draft_id)?;
        Self::read_file(&self.dir_for(logbook_id).join(format!("{draft_id}.json")))
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Draft not found"))
    }

    pub fn delete(&self, logbook_id: &str, draft_id: &str) -> AppResult<()> {
        check_id("logbook_id", logbook_id)?;
        check_id("draft_id", draft_id)?;
        let p = self.dir_for(logbook_id).join(format!("{draft_id}.json"));
        match fs::remove_file(&p) {
            Ok(()) => {
                let _ = fsutil::sync_dir(&self.dir_for(logbook_id));
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Staging ids referenced by drafts of a logbook, so orphan cleanup keeps their files.
    pub fn referenced_staging(&self, logbook_id: &str) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        for d in self.list(logbook_id).unwrap_or_default() {
            if let Some(arr) = d.content.get("new_attachments").and_then(|v| v.as_array()) {
                for a in arr {
                    if let Some(s) = a.get("staging_id").and_then(|v| v.as_str()) {
                        set.insert(s.to_string());
                    }
                }
            }
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(lb: &str) -> DraftMeta {
        DraftMeta {
            draft_id: fsutil::new_uuid(),
            logbook_id: lb.into(),
            logbook_name: "Test".into(),
            editor: "new".into(),
            entry_id: None,
            entry_sequence: None,
            base_revision: None,
        }
    }

    #[test]
    fn save_list_delete_and_checksum() {
        let d = tempfile::tempdir().unwrap();
        let store = DraftStore::new(d.path());
        let lb = fsutil::new_uuid();
        let m = meta(&lb);
        store.save(&m, &serde_json::json!({"subject": "half typed"})).unwrap();
        let list = store.list(&lb).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].intact);
        // Tamper with the content: still listed, flagged as not intact.
        let p = d.path().join(&lb).join(format!("{}.json", m.draft_id));
        let s = fs::read_to_string(&p).unwrap().replace("half typed", "tampered");
        fs::write(&p, s).unwrap();
        assert!(!store.list(&lb).unwrap()[0].intact);
        store.delete(&lb, &m.draft_id).unwrap();
        assert!(store.list(&lb).unwrap().is_empty());
    }

    #[test]
    fn rejects_traversal_ids() {
        let d = tempfile::tempdir().unwrap();
        let store = DraftStore::new(d.path());
        let mut m = meta(&fsutil::new_uuid());
        m.draft_id = "../../evil".into();
        assert!(store.save(&m, &serde_json::json!({})).is_err());
    }
}
