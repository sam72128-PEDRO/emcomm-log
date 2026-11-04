//! Per-logbook station directory (2.2 DIR-*). Local to the logbook, so it travels in `.emlog` packages and
//! never leaks between logbooks. Not an operator profile and not the logbook's own station: choosing a
//! directory item only copies text into a new entry, and editing the directory never rewrites entries.

use crate::error::{AppError, AppResult};
use crate::fsutil::new_uuid;
use crate::fsutil::now_utc;
use crate::logbook::Logbook;
use crate::validate::{multi_line, normalize_callsign, single_line};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub const MAX_ALIASES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryKind {
    AmateurCallsign,
    TacticalIdentifier,
    Station,
    Organization,
    #[default]
    Other,
}

impl DirectoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AmateurCallsign => "amateur_callsign",
            Self::TacticalIdentifier => "tactical_identifier",
            Self::Station => "station",
            Self::Organization => "organization",
            Self::Other => "other",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "amateur_callsign" => Self::AmateurCallsign,
            "tactical_identifier" => Self::TacticalIdentifier,
            "station" => Self::Station,
            "organization" => Self::Organization,
            "other" => Self::Other,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct DirectoryInput {
    pub label: String,
    pub kind: DirectoryKind,
    pub identifier: String,
    pub agency: String,
    pub channel: String,
    pub frequency_hz: Option<i64>,
    pub mode: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirectoryEntry {
    pub id: String,
    pub label: String,
    pub kind: DirectoryKind,
    pub identifier: String,
    pub agency: String,
    pub channel: String,
    pub frequency_hz: Option<i64>,
    pub mode: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub notes: String,
    pub last_used_utc: Option<String>,
    pub last_entry_id: Option<String>,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirectorySaveResult {
    pub entry: DirectoryEntry,
    /// Other directory items with the same identifier (a warning, not a block: two meanings may share one).
    pub similar: Vec<DirectoryEntry>,
}

/// Search/duplicate key: trimmed, internal whitespace collapsed, upper-cased.
pub fn normalize_key(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase()
}

fn validate(i: &DirectoryInput) -> AppResult<DirectoryInput> {
    let label = single_line("directory.label", &i.label, 120)?;
    if label.is_empty() {
        return Err(AppError::validation("directory.label", "Enter a name to show for this station"));
    }
    let identifier = match i.kind {
        DirectoryKind::AmateurCallsign => normalize_callsign("directory.identifier", &i.identifier)?,
        _ => single_line("directory.identifier", &i.identifier, 60)?,
    };
    if i.frequency_hz.is_some_and(|f| f <= 0) {
        return Err(AppError::validation("directory.frequency_hz", "Frequency must be above zero"));
    }
    let mut aliases: Vec<String> = Vec::new();
    for a in &i.aliases {
        let a = single_line("directory.aliases", a, 60)?;
        if !a.is_empty() && !aliases.iter().any(|x| normalize_key(x) == normalize_key(&a)) {
            aliases.push(a);
        }
    }
    if aliases.len() > MAX_ALIASES {
        return Err(AppError::validation("directory.aliases", format!("At most {MAX_ALIASES} aliases")));
    }
    let mut tags: Vec<String> = Vec::new();
    for t in &i.tags {
        let t = single_line("directory.tags", t, 40)?.to_uppercase();
        if !t.is_empty() && !tags.contains(&t) {
            tags.push(t);
        }
    }
    Ok(DirectoryInput {
        label,
        kind: i.kind,
        identifier,
        agency: single_line("directory.agency", &i.agency, 200)?,
        channel: single_line("directory.channel", &i.channel, 120)?,
        frequency_hz: i.frequency_hz,
        mode: single_line("directory.mode", &i.mode, 20)?.to_uppercase(),
        aliases,
        tags,
        notes: multi_line("directory.notes", &i.notes, 2000)?,
    })
}

const SELECT: &str = "SELECT id, label, kind, identifier, agency, channel, frequency_hz, mode, aliases_json, tags_json, notes,
    last_used_utc, last_entry_id, created_at_utc, updated_at_utc FROM station_directory";

fn row(r: &rusqlite::Row) -> rusqlite::Result<DirectoryEntry> {
    let kind: String = r.get(2)?;
    let aliases: String = r.get(8)?;
    let tags: String = r.get(9)?;
    Ok(DirectoryEntry {
        id: r.get(0)?,
        label: r.get(1)?,
        kind: DirectoryKind::parse(&kind).unwrap_or_default(),
        identifier: r.get(3)?,
        agency: r.get(4)?,
        channel: r.get(5)?,
        frequency_hz: r.get(6)?,
        mode: r.get(7)?,
        aliases: serde_json::from_str(&aliases).unwrap_or_default(),
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        notes: r.get(10)?,
        last_used_utc: r.get(11)?,
        last_entry_id: r.get(12)?,
        created_at_utc: r.get(13)?,
        updated_at_utc: r.get(14)?,
    })
}

impl Logbook {
    /// All directory items matching `query` (label, identifier, alias, agency, tag; case-insensitive),
    /// most recently used first, then by label. Empty query lists everything.
    pub fn list_directory(&self, query: &str) -> AppResult<Vec<DirectoryEntry>> {
        let mut st = self.conn.prepare(&format!("{SELECT} ORDER BY last_used_utc IS NULL, last_used_utc DESC, label COLLATE NOCASE"))?;
        let all: Vec<DirectoryEntry> = st.query_map([], row)?.collect::<Result<_, _>>()?;
        let q = normalize_key(query);
        if q.is_empty() {
            return Ok(all);
        }
        Ok(all
            .into_iter()
            .filter(|d| {
                std::iter::once(&d.label)
                    .chain(std::iter::once(&d.identifier))
                    .chain(std::iter::once(&d.agency))
                    .chain(d.aliases.iter())
                    .chain(d.tags.iter())
                    .any(|v| normalize_key(v).contains(&q))
            })
            .collect())
    }

    pub fn directory_entry(&self, id: &str) -> AppResult<DirectoryEntry> {
        self.conn
            .query_row(&format!("{SELECT} WHERE id=?1"), params![id], row)
            .optional()?
            .ok_or_else(|| AppError::new(crate::error::ErrorCode::NotFound, "That directory item no longer exists"))
    }

    /// Create (`id` = None) or update a directory item. Saved immediately; never touches entries.
    pub fn save_directory(&mut self, id: Option<&str>, input: &DirectoryInput) -> AppResult<DirectorySaveResult> {
        self.ensure_writable()?;
        let v = validate(input)?;
        let now = now_utc();
        let norm = normalize_key(&v.identifier);
        let tx = self.conn.transaction()?;
        let id = match id {
            Some(id) => {
                let n = tx.execute(
                    "UPDATE station_directory SET label=?2, kind=?3, identifier=?4, identifier_norm=?5, agency=?6, channel=?7,
                        frequency_hz=?8, mode=?9, aliases_json=?10, tags_json=?11, notes=?12, updated_at_utc=?13 WHERE id=?1",
                    params![
                        id,
                        v.label,
                        v.kind.as_str(),
                        v.identifier,
                        norm,
                        v.agency,
                        v.channel,
                        v.frequency_hz,
                        v.mode,
                        serde_json::to_string(&v.aliases)?,
                        serde_json::to_string(&v.tags)?,
                        v.notes,
                        now
                    ],
                )?;
                if n == 0 {
                    return Err(AppError::new(crate::error::ErrorCode::NotFound, "That directory item no longer exists"));
                }
                id.to_string()
            }
            None => {
                let id = new_uuid();
                tx.execute(
                    "INSERT INTO station_directory(id, label, kind, identifier, identifier_norm, agency, channel, frequency_hz,
                        mode, aliases_json, tags_json, notes, created_at_utc, updated_at_utc)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",
                    params![
                        id,
                        v.label,
                        v.kind.as_str(),
                        v.identifier,
                        norm,
                        v.agency,
                        v.channel,
                        v.frequency_hz,
                        v.mode,
                        serde_json::to_string(&v.aliases)?,
                        serde_json::to_string(&v.tags)?,
                        v.notes,
                        now
                    ],
                )?;
                id
            }
        };
        tx.commit()?;
        let entry = self.directory_entry(&id)?;
        let similar = if norm.is_empty() {
            vec![]
        } else {
            let mut st = self.conn.prepare(&format!("{SELECT} WHERE identifier_norm=?1 AND id<>?2"))?;
            st.query_map(params![norm, id], row)?.collect::<Result<_, _>>()?
        };
        Ok(DirectorySaveResult { entry, similar })
    }

    /// Remove a directory item. Entries that used it keep their own copied text.
    pub fn delete_directory(&mut self, id: &str) -> AppResult<()> {
        self.ensure_writable()?;
        self.conn.execute("DELETE FROM station_directory WHERE id=?1", params![id])?;
        Ok(())
    }

    /// Record that an item was used for a saved entry (ranks it first in suggestions).
    pub fn mark_directory_used(&mut self, id: &str, entry_id: &str) -> AppResult<()> {
        self.ensure_writable()?;
        self.conn
            .execute("UPDATE station_directory SET last_used_utc=?2, last_entry_id=?3 WHERE id=?1", params![id, now_utc(), entry_id])?;
        Ok(())
    }
}
