//! Fixed tofu payload env per task kind (T03 baseline).
//!
//! Pure data like [`tofu_payload_argv`](crate::argv::tofu_payload_argv):
//! the orchestrator threads the returned pairs through the pinned-tool
//! execution env. Every kind carries the automation pair; per-root
//! data/config/cache paths are a later task's isolated env.

use std::ffi::OsString;

use crate::kinds::TofuTaskKind;

/// Automation-marker env key.
pub const TF_IN_AUTOMATION_ENV: &str = "TF_IN_AUTOMATION";
/// Automation marker enabled.
pub const TF_IN_AUTOMATION_ON: &str = "1";
/// Interactive-input env key.
pub const TF_INPUT_ENV: &str = "TF_INPUT";
/// Interactive input disabled.
pub const TF_INPUT_OFF: &str = "0";

/// Fixed payload env for one task kind.
///
/// Every kind carries `TF_IN_AUTOMATION=1 TF_INPUT=0` (T03 baseline
/// automation pair).
#[must_use]
pub fn tofu_payload_env(kind: TofuTaskKind) -> Vec<(OsString, OsString)> {
    let _ = kind;
    vec![
        (
            OsString::from(TF_IN_AUTOMATION_ENV),
            OsString::from(TF_IN_AUTOMATION_ON),
        ),
        (OsString::from(TF_INPUT_ENV), OsString::from(TF_INPUT_OFF)),
    ]
}
