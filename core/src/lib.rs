//! EMCOMM LOG core: all authoritative persistence, validation, backup, package, and export logic.
//! The Tauri shell is a thin command layer over this crate.

pub mod appdb;
pub mod backup;
pub mod drafts;
pub mod entries;
pub mod error;
pub mod export;
pub mod faults;
pub mod fsutil;
pub mod gps;
pub mod logbook;
pub mod migrate;
pub mod models;
pub mod package;
pub mod periods;
pub mod recents;
pub mod snapshot;
pub mod station;
pub mod validate;

pub use error::{AppError, AppResult, ErrorCode};

/// Re-exported so the app shell can name connection types without its own (possibly mismatched) dependency.
pub use rusqlite;
