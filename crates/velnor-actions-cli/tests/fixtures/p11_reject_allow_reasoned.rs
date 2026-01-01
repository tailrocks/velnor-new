//! Negative fixture: reasoned `allow` (fires `allow_attributes = "warn"`,
//! so `-D warnings` denies every allow, reasoned or not).

/// Suppression with a reason; still rejected once warnings are denied.
#[allow(dead_code, reason = "fixture proves the warn fires")]
pub fn never_called() -> u32 {
    7
}
