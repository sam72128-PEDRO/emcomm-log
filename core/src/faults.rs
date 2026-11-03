//! Test-only fault injection. Compiled to no-ops unless the `fault-injection` feature is on,
//! so release builds carry no way to trigger synthetic failures.

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    DiskFull,
    PermissionDenied,
    Io,
    Busy,
    /// Simulates the process dying at this point: the call panics.
    Crash,
}

#[cfg(feature = "fault-injection")]
mod imp {
    use super::Fault;
    use crate::error::{AppError, AppResult, ErrorCode};
    use std::cell::RefCell;
    use std::collections::HashMap;

    // Thread-local so parallel tests cannot consume each other's injected faults.
    thread_local! {
        static FAULTS: RefCell<HashMap<String, (Fault, u32)>> = RefCell::new(HashMap::new());
    }

    pub fn inject(point: &str, fault: Fault, times: u32) {
        FAULTS.with(|f| f.borrow_mut().insert(point.to_string(), (fault, times)));
    }

    pub fn clear() {
        FAULTS.with(|f| f.borrow_mut().clear());
    }

    pub fn check(point: &str) -> AppResult<()> {
        let fault = FAULTS.with(|f| {
            let mut map = f.borrow_mut();
            let entry = map.get_mut(point)?;
            let fault = entry.0;
            entry.1 = entry.1.saturating_sub(1);
            if entry.1 == 0 {
                map.remove(point);
            }
            Some(fault)
        });
        let Some(fault) = fault else { return Ok(()) };
        let err = match fault {
            Fault::DiskFull => AppError::new(ErrorCode::DiskFull, "The disk holding the logbook is full (injected)"),
            Fault::PermissionDenied => AppError::new(ErrorCode::PermissionDenied, "Permission denied (injected)"),
            Fault::Io => AppError::new(ErrorCode::Io, "Disk I/O error (injected)"),
            Fault::Busy => AppError::new(ErrorCode::Busy, "Database busy (injected)"),
            Fault::Crash => panic!("injected crash at {point}"),
        };
        Err(err)
    }
}

#[cfg(feature = "fault-injection")]
pub use imp::{clear, inject};

#[inline]
pub fn check(_point: &str) -> AppResult<()> {
    #[cfg(feature = "fault-injection")]
    {
        imp::check(_point)
    }
    #[cfg(not(feature = "fault-injection"))]
    {
        Ok(())
    }
}
