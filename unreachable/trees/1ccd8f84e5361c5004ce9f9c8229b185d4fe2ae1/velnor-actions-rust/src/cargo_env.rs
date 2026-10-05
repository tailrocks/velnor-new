//! Fixed Cargo payload env per task kind (task §2, rust §6).
//!
//! Pure data like [`cargo_payload_argv`](crate::tasks::cargo_payload_argv):
//! the orchestrator threads the returned pairs through the pinned-tool
//! execution env. Only documentation builds mandate env today.

use std::ffi::OsString;

use crate::tasks::TaskKind;

/// `RUSTDOCFLAGS` env key denying documentation warnings.
pub const RUSTDOCFLAGS_ENV: &str = "RUSTDOCFLAGS";
/// Documentation warnings denied as errors.
pub const DENY_WARNINGS: &str = "-D warnings";

/// Fixed payload env for one task kind.
///
/// [`TaskKind::Doc`] carries `RUSTDOCFLAGS=-D warnings` so rustdoc
/// warnings fail the build; every other kind carries no env.
#[must_use]
pub fn cargo_payload_env(kind: TaskKind) -> Vec<(OsString, OsString)> {
    if kind == TaskKind::Doc {
        vec![(
            OsString::from(RUSTDOCFLAGS_ENV),
            OsString::from(DENY_WARNINGS),
        )]
    } else {
        Vec::new()
    }
}
