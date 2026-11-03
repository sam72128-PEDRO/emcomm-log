use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{now_utc, sha256_bytes};
use rusqlite::{params, Connection, OptionalExtension};

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub up: &'static str,
    pub down: &'static str,
}

pub const LOGBOOK_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "init",
    up: include_str!("../migrations/logbook/0001_init.up.sql"),
    down: include_str!("../migrations/logbook/0001_init.down.sql"),
}];

pub const APP_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "init",
    up: include_str!("../migrations/app/0001_init.up.sql"),
    down: include_str!("../migrations/app/0001_init.down.sql"),
}];

pub fn latest_version(set: &[Migration]) -> u32 {
    set.iter().map(|m| m.version).max().unwrap_or(0)
}

fn ensure_table(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            checksum TEXT NOT NULL,
            applied_at_utc TEXT NOT NULL
        );",
    )?;
    Ok(())
}

pub fn current_version(conn: &Connection) -> AppResult<u32> {
    let exists: Option<String> = conn
        .query_row("SELECT name FROM sqlite_master WHERE type='table' AND name='schema_migrations'", [], |r| r.get(0))
        .optional()?;
    if exists.is_none() {
        return Ok(0);
    }
    let v: Option<u32> = conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))?;
    Ok(v.unwrap_or(0))
}

/// Verify recorded checksums and apply pending migrations, each in its own transaction.
/// Refuses databases created by a newer schema than this build understands.
pub fn migrate(conn: &mut Connection, set: &[Migration]) -> AppResult<u32> {
    ensure_table(conn)?;
    let latest = latest_version(set);
    let current = current_version(conn)?;
    if current > latest {
        return Err(AppError::new(
            ErrorCode::UnsupportedVersion,
            format!("This database uses schema version {current}, newer than this app supports ({latest}). Update EMCOMM LOG to open it."),
        ));
    }
    {
        let mut stmt = conn.prepare("SELECT version, checksum FROM schema_migrations ORDER BY version")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (v, sum) = row?;
            if let Some(m) = set.iter().find(|m| m.version == v) {
                if sha256_bytes(m.up.as_bytes()) != sum {
                    return Err(AppError::new(
                        ErrorCode::Integrity,
                        format!("Schema migration {v} checksum mismatch; the database was modified by an incompatible build"),
                    ));
                }
            }
        }
    }
    for m in set.iter().filter(|m| m.version > current) {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(m.up)?;
        tx.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at_utc) VALUES (?1, ?2, ?3, ?4)",
            params![m.version, m.name, sha256_bytes(m.up.as_bytes()), now_utc()],
        )?;
        tx.commit()?;
    }
    Ok(latest)
}

/// Roll back to `target` version using the down scripts (used by migration tests and tooling).
pub fn rollback(conn: &mut Connection, set: &[Migration], target: u32) -> AppResult<()> {
    let current = current_version(conn)?;
    for m in set.iter().rev().filter(|m| m.version > target && m.version <= current) {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(m.down)?;
        tx.execute("DELETE FROM schema_migrations WHERE version = ?1", params![m.version])?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logbook_migrations_up_down_up() {
        let mut c = Connection::open_in_memory().unwrap();
        assert_eq!(migrate(&mut c, LOGBOOK_MIGRATIONS).unwrap(), 1);
        rollback(&mut c, LOGBOOK_MIGRATIONS, 0).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='entries'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        migrate(&mut c, LOGBOOK_MIGRATIONS).unwrap();
        assert_eq!(current_version(&c).unwrap(), 1);
    }

    #[test]
    fn rejects_newer_schema() {
        let mut c = Connection::open_in_memory().unwrap();
        migrate(&mut c, APP_MIGRATIONS).unwrap();
        c.execute("INSERT INTO schema_migrations VALUES (99, 'future', 'x', 'now')", []).unwrap();
        let e = migrate(&mut c, APP_MIGRATIONS).unwrap_err();
        assert_eq!(e.code, ErrorCode::UnsupportedVersion);
    }

    #[test]
    fn detects_checksum_tamper() {
        let mut c = Connection::open_in_memory().unwrap();
        migrate(&mut c, APP_MIGRATIONS).unwrap();
        c.execute("UPDATE schema_migrations SET checksum='bad' WHERE version=1", []).unwrap();
        assert_eq!(migrate(&mut c, APP_MIGRATIONS).unwrap_err().code, ErrorCode::Integrity);
    }
}
