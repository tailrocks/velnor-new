//! Test-only Cargo boundary observation without launching fake executables.

use std::cell::Cell;

thread_local! {
    static ATTEMPTS: Cell<Option<usize>> = const { Cell::new(None) };
}

pub(crate) struct CargoProbe;

impl CargoProbe {
    pub(crate) fn begin() -> Self {
        ATTEMPTS.with(|attempts| attempts.set(Some(0)));
        Self
    }

    pub(crate) fn attempts(&self) -> usize {
        ATTEMPTS.with(|attempts| attempts.get().unwrap_or_default())
    }
}

impl Drop for CargoProbe {
    fn drop(&mut self) {
        ATTEMPTS.with(|attempts| attempts.set(None));
    }
}

/// Fail before a subprocess whenever a test requires Cargo to remain unused.
pub(super) fn record_attempt() -> Result<(), String> {
    ATTEMPTS.with(|attempts| match attempts.get() {
        Some(count) => {
            attempts.set(Some(count + 1));
            Err("unexpected_cargo_invocation".to_owned())
        }
        None => Ok(()),
    })
}
