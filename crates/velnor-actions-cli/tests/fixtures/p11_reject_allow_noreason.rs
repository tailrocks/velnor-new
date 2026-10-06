//! Negative fixture: reason-less `allow` (rejected by
//! `allow_attributes_without_reason = "deny"`).

/// Violating suppression: no `reason` recorded for the allow.
#[allow(dead_code)]
pub fn never_called() -> u32 {
    7
}
