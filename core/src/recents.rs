//! Local suggestions from this logbook's own recent history (2.1 spec §5.3). Read-only, bounded to the
//! most recent entries so it stays fast on large logbooks and low-end laptops; never a network lookup.
//! Suggestions are offered in the UI and only applied when the operator picks one.

use crate::error::{AppError, AppResult};
use crate::logbook::Logbook;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How many of the newest active entries are considered.
pub const WINDOW: i64 = 2000;
/// Most values returned per field.
pub const CAP: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RecentField {
    TheirCall,
    Station,
    Channel,
    FrequencyHz,
    Mode,
    PotaRef,
    SotaRef,
}

impl RecentField {
    pub const ALL: [RecentField; 7] =
        [Self::TheirCall, Self::Station, Self::Channel, Self::FrequencyHz, Self::Mode, Self::PotaRef, Self::SotaRef];
}

impl Logbook {
    /// Distinct non-empty values per field, most recently used first, then most used.
    pub fn recent_values(&self, fields: &[RecentField]) -> AppResult<BTreeMap<RecentField, Vec<String>>> {
        let mut out = BTreeMap::new();
        for &f in fields {
            // `w` = the newest WINDOW active entries with a recency rank (1 = newest). The per-field SQL
            // is fixed text, never built from caller input.
            let ranked = format!(
                "WITH w AS (SELECT id, from_text, to_text, channel, \
                   ROW_NUMBER() OVER (ORDER BY recorded_at_utc DESC, sequence_no DESC) AS rk \
                   FROM entries WHERE status = 'active' ORDER BY recorded_at_utc DESC, sequence_no DESC LIMIT {WINDOW}) \
                 SELECT val, MIN(rk) AS last_rk, COUNT(*) AS n FROM ({}) \
                 WHERE val IS NOT NULL AND TRIM(val) <> '' GROUP BY val ORDER BY last_rk, n DESC LIMIT {CAP}",
                ranked_sql(f)
            );
            let mut st = self.conn.prepare(&ranked).map_err(AppError::from)?;
            let vals = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
            out.insert(f, vals);
        }
        Ok(out)
    }
}

fn ranked_sql(f: RecentField) -> &'static str {
    use RecentField::*;
    match f {
        TheirCall => "SELECT h.their_call AS val, w.rk FROM w JOIN entry_ham h ON h.entry_id = w.id",
        Station => "SELECT from_text AS val, rk FROM w UNION ALL SELECT to_text, rk FROM w",
        Channel => "SELECT channel AS val, rk FROM w",
        FrequencyHz => "SELECT CAST(h.frequency_hz AS TEXT) AS val, w.rk FROM w JOIN entry_ham h ON h.entry_id = w.id",
        Mode => "SELECT h.mode AS val, w.rk FROM w JOIN entry_ham h ON h.entry_id = w.id",
        PotaRef => {
            "SELECT h.pota_ref AS val, w.rk FROM w JOIN entry_ham h ON h.entry_id = w.id UNION ALL \
             SELECT h.my_pota_ref, w.rk FROM w JOIN entry_ham h ON h.entry_id = w.id"
        }
        SotaRef => {
            "SELECT h.sota_ref AS val, w.rk FROM w JOIN entry_ham h ON h.entry_id = w.id UNION ALL \
             SELECT h.my_sota_ref, w.rk FROM w JOIN entry_ham h ON h.entry_id = w.id"
        }
    }
}
