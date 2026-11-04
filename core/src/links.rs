//! Related communications (2.2 REL-*): links between two entries of the same logbook. A link creates no
//! entry and changes neither entry. Links are never deleted; removing one records who and when on the row
//! (the audit trail), and voiding an entry leaves its links in place, shown as voided.

use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{new_uuid, now_utc};
use crate::logbook::Logbook;
use crate::models::{EntryKind, EntryStatus, OperatorSnapshot};
use crate::validate::single_line;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// Symmetric: stored once, in canonical (smaller id first) order.
    RelatedTo,
    FollowUpTo,
    RelayOf,
    ResponseTo,
    /// A new communication that corrects an earlier one (a typo in the same entry is an Edit).
    Corrects,
}

impl Relation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RelatedTo => "related_to",
            Self::FollowUpTo => "follow_up_to",
            Self::RelayOf => "relay_of",
            Self::ResponseTo => "response_to",
            Self::Corrects => "corrects",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "related_to" => Self::RelatedTo,
            "follow_up_to" => Self::FollowUpTo,
            "relay_of" => Self::RelayOf,
            "response_to" => Self::ResponseTo,
            "corrects" => Self::Corrects,
            _ => return None,
        })
    }
    pub fn symmetric(&self) -> bool {
        matches!(self, Self::RelatedTo)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryLink {
    pub id: String,
    pub source_entry_id: String,
    pub target_entry_id: String,
    pub relation: Relation,
    pub note: String,
    pub created_at_utc: String,
    pub created_by: OperatorSnapshot,
    pub removed_at_utc: Option<String>,
    pub removed_by: Option<OperatorSnapshot>,
}

/// The other end of a link, as seen from one entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkCounterpart {
    pub id: String,
    pub sequence_no: i64,
    pub kind: EntryKind,
    pub event_at_utc: String,
    pub subject: String,
    pub status: EntryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkView {
    pub link: EntryLink,
    /// True when the viewed entry is the link's source ("this entry is a relay of #8").
    pub outgoing: bool,
    pub counterpart: LinkCounterpart,
}

const LINK_SELECT: &str = "SELECT l.id, l.source_entry_id, l.target_entry_id, l.relation, l.note, l.created_at_utc,
    l.created_by_json, l.removed_at_utc, l.removed_by_json FROM entry_links l";

fn link_row(r: &rusqlite::Row) -> rusqlite::Result<EntryLink> {
    let rel: String = r.get(3)?;
    let by: String = r.get(6)?;
    let rby: Option<String> = r.get(8)?;
    Ok(EntryLink {
        id: r.get(0)?,
        source_entry_id: r.get(1)?,
        target_entry_id: r.get(2)?,
        relation: Relation::parse(&rel).unwrap_or(Relation::RelatedTo),
        note: r.get(4)?,
        created_at_utc: r.get(5)?,
        created_by: serde_json::from_str(&by).unwrap_or_default(),
        removed_at_utc: r.get(7)?,
        removed_by: rby.and_then(|j| serde_json::from_str(&j).ok()),
    })
}

fn counterpart(conn: &rusqlite::Connection, id: &str) -> AppResult<Option<LinkCounterpart>> {
    Ok(conn
        .query_row("SELECT id, sequence_no, kind, event_at_utc, subject, status FROM entries WHERE id=?1", params![id], |r| {
            let kind: String = r.get(2)?;
            let status: String = r.get(5)?;
            Ok(LinkCounterpart {
                id: r.get(0)?,
                sequence_no: r.get(1)?,
                kind: EntryKind::parse(&kind).unwrap_or(EntryKind::Note),
                event_at_utc: r.get(3)?,
                subject: r.get(4)?,
                status: EntryStatus::parse(&status).unwrap_or(EntryStatus::Active),
            })
        })
        .optional()?)
}

impl Logbook {
    /// Link `source` to `target`. Rejects self-links, missing entries, an existing identical link (in
    /// either order for `related_to`) and the immediate loop "A relay of B" + "B relay of A".
    pub fn add_link(
        &mut self,
        source_id: &str,
        target_id: &str,
        relation: Relation,
        note: &str,
        editor: &OperatorSnapshot,
    ) -> AppResult<LinkView> {
        self.ensure_writable()?;
        let note = single_line("link.note", note, 200)?;
        if source_id == target_id {
            return Err(AppError::validation("link.target", "An entry cannot be linked to itself"));
        }
        let (src, dst) = if relation.symmetric() && target_id < source_id { (target_id, source_id) } else { (source_id, target_id) };
        let tx = self.conn.transaction()?;
        for id in [src, dst] {
            if counterpart(&tx, id)?.is_none() {
                return Err(AppError::new(ErrorCode::NotFound, "That entry is not in this logbook"));
            }
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM entry_links WHERE removed_at_utc IS NULL AND relation=?3
                AND source_entry_id=?1 AND target_entry_id=?2)",
            params![src, dst, relation.as_str()],
            |r| r.get(0),
        )?;
        if exists {
            return Err(AppError::validation("link.target", "These entries are already linked that way"));
        }
        if !relation.symmetric() {
            let reverse: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM entry_links WHERE removed_at_utc IS NULL AND relation=?3
                    AND source_entry_id=?2 AND target_entry_id=?1)",
                params![src, dst, relation.as_str()],
                |r| r.get(0),
            )?;
            if reverse {
                return Err(AppError::validation(
                    "link.target",
                    "The other entry is already linked this way to this one; a link cannot point both ways",
                ));
            }
        }
        let id = new_uuid();
        tx.execute(
            "INSERT INTO entry_links(id, source_entry_id, target_entry_id, relation, note, created_at_utc, created_by_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![id, src, dst, relation.as_str(), note, now_utc(), serde_json::to_string(editor)?],
        )?;
        tx.commit()?;
        let link = self.conn.query_row(&format!("{LINK_SELECT} WHERE l.id=?1"), params![id], link_row)?;
        let outgoing = link.source_entry_id == source_id;
        let other = if outgoing { &link.target_entry_id } else { &link.source_entry_id };
        let counterpart = counterpart(&self.conn, other)?.expect("checked above");
        Ok(LinkView { link, outgoing, counterpart })
    }

    /// Remove a link: recorded on the link (who, when); neither entry changes.
    pub fn remove_link(&mut self, link_id: &str, editor: &OperatorSnapshot) -> AppResult<()> {
        self.ensure_writable()?;
        let n = self.conn.execute(
            "UPDATE entry_links SET removed_at_utc=?2, removed_by_json=?3 WHERE id=?1 AND removed_at_utc IS NULL",
            params![link_id, now_utc(), serde_json::to_string(editor)?],
        )?;
        if n == 0 {
            return Err(AppError::new(ErrorCode::NotFound, "That link was already removed"));
        }
        Ok(())
    }

    /// One hop: every link touching `entry_id`, newest first; removed links only when asked.
    pub fn entry_links(&self, entry_id: &str, include_removed: bool) -> AppResult<Vec<LinkView>> {
        let mut st = self.conn.prepare(&format!(
            "{LINK_SELECT} WHERE (l.source_entry_id=?1 OR l.target_entry_id=?1) {} ORDER BY l.created_at_utc DESC",
            if include_removed { "" } else { "AND l.removed_at_utc IS NULL" }
        ))?;
        let links: Vec<EntryLink> = st.query_map(params![entry_id], link_row)?.collect::<Result<_, _>>()?;
        let mut out = Vec::with_capacity(links.len());
        for link in links {
            let outgoing = link.source_entry_id == entry_id;
            let other = if outgoing { link.target_entry_id.clone() } else { link.source_entry_id.clone() };
            if let Some(counterpart) = counterpart(&self.conn, &other)? {
                out.push(LinkView { link, outgoing, counterpart });
            }
        }
        Ok(out)
    }
}
