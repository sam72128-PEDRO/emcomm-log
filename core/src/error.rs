use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Validation,
    NotFound,
    Conflict,
    StorageUnavailable,
    DiskFull,
    PermissionDenied,
    Busy,
    Integrity,
    Io,
    Locked,
    UnsupportedVersion,
    UnsupportedLocation,
    InvalidPackage,
    NoLogbookOpen,
    ReadOnly,
    AlreadyExists,
    Cancelled,
    Internal,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::Validation => "VALIDATION",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::Conflict => "CONFLICT",
            ErrorCode::StorageUnavailable => "STORAGE_UNAVAILABLE",
            ErrorCode::DiskFull => "DISK_FULL",
            ErrorCode::PermissionDenied => "PERMISSION_DENIED",
            ErrorCode::Busy => "BUSY",
            ErrorCode::Integrity => "INTEGRITY",
            ErrorCode::Io => "IO",
            ErrorCode::Locked => "LOCKED",
            ErrorCode::UnsupportedVersion => "UNSUPPORTED_VERSION",
            ErrorCode::UnsupportedLocation => "UNSUPPORTED_LOCATION",
            ErrorCode::InvalidPackage => "INVALID_PACKAGE",
            ErrorCode::NoLogbookOpen => "NO_LOGBOOK_OPEN",
            ErrorCode::ReadOnly => "READ_ONLY",
            ErrorCode::AlreadyExists => "ALREADY_EXISTS",
            ErrorCode::Cancelled => "CANCELLED",
            ErrorCode::Internal => "INTERNAL",
        }
    }

    /// Errors that mean the live storage location itself cannot be trusted for writes.
    pub fn is_storage_fault(&self) -> bool {
        matches!(
            self,
            ErrorCode::StorageUnavailable
                | ErrorCode::DiskFull
                | ErrorCode::PermissionDenied
                | ErrorCode::Io
                | ErrorCode::Integrity
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        AppError { code, message: message.into(), field: None }
    }
    pub fn validation(field: &str, message: impl Into<String>) -> Self {
        AppError { code: ErrorCode::Validation, message: message.into(), field: Some(field.to_string()) }
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }
    pub fn with_context(mut self, context: &str) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        use rusqlite::ffi::ErrorCode as C;
        match &e {
            rusqlite::Error::SqliteFailure(err, msg) => {
                let detail = msg.clone().unwrap_or_else(|| err.to_string());
                let code = match err.code {
                    C::DiskFull => ErrorCode::DiskFull,
                    C::DatabaseBusy | C::DatabaseLocked => ErrorCode::Busy,
                    C::CannotOpen => ErrorCode::StorageUnavailable,
                    C::ReadOnly | C::PermissionDenied | C::AuthorizationForStatementDenied => {
                        ErrorCode::PermissionDenied
                    }
                    C::SystemIoFailure => ErrorCode::Io,
                    C::DatabaseCorrupt | C::NotADatabase => ErrorCode::Integrity,
                    C::ConstraintViolation => ErrorCode::Conflict,
                    _ => ErrorCode::Internal,
                };
                let friendly = match code {
                    ErrorCode::DiskFull => "The disk holding the logbook is full".to_string(),
                    ErrorCode::Busy => "The logbook database is busy or locked by another process".to_string(),
                    ErrorCode::StorageUnavailable => "The logbook database file cannot be opened".to_string(),
                    ErrorCode::PermissionDenied => "Permission denied writing the logbook database".to_string(),
                    ErrorCode::Io => "A disk I/O error occurred writing the logbook database".to_string(),
                    ErrorCode::Integrity => "The logbook database is damaged or not a logbook".to_string(),
                    _ => format!("Database error: {detail}"),
                };
                AppError::new(code, friendly)
            }
            rusqlite::Error::QueryReturnedNoRows => AppError::not_found("Record not found"),
            _ => AppError::internal(format!("Database error: {e}")),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind as K;
        let code = match e.kind() {
            K::NotFound => ErrorCode::StorageUnavailable,
            K::PermissionDenied | K::ReadOnlyFilesystem => ErrorCode::PermissionDenied,
            K::StorageFull | K::QuotaExceeded => ErrorCode::DiskFull,
            K::AlreadyExists => ErrorCode::AlreadyExists,
            K::WouldBlock => ErrorCode::Locked,
            _ => ErrorCode::Io,
        };
        let message = match code {
            ErrorCode::DiskFull => "The destination disk is full".to_string(),
            ErrorCode::PermissionDenied => "Permission denied".to_string(),
            ErrorCode::StorageUnavailable => "File or folder not found (drive disconnected or moved?)".to_string(),
            _ => format!("File system error: {e}"),
        };
        AppError::new(code, message)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::internal(format!("JSON error: {e}"))
    }
}
