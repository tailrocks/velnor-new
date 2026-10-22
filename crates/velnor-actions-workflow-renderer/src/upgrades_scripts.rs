//! Job scripts of `upgrades.yml`, composed exactly as they are rendered.
//!
//! Every script that builds, checks or applies the upgrade patch starts with
//! the shared patch filter (`upgrades_patch_filter.sh`), so prepare and
//! publish enforce one rule set.

pub(super) const FILTER: &str = include_str!("upgrades_patch_filter.sh");
const PREPARE: &str = include_str!("upgrades_prepare.sh");
const PUBLISH: &str = include_str!("upgrades_publish.sh");
pub(super) const GATE: &str = include_str!("upgrades_gate.sh");

/// A job script with the shared patch filter's functions defined first.
fn with_filter(script: &str) -> String {
    format!("{FILTER}\n{script}")
}

/// The prepare job's bump script.
pub(super) fn prepare_script() -> String {
    with_filter(PREPARE)
}

/// The publish job's push script.
pub(super) fn publish_script() -> String {
    with_filter(PUBLISH)
}
