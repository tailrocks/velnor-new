//! Job scripts of `upgrades.yml`, composed exactly as they are rendered.
//!
//! Every script that builds, checks or applies the upgrade patch starts with
//! the shared patch filter (`upgrades_patch_filter.sh`), so prepare, verify
//! and publish enforce one rule set; the two that run `velnor-actions
//! generate` share its prerequisites (`upgrades_generate.sh`).

pub(super) const FILTER: &str = include_str!("upgrades_patch_filter.sh");
const GENERATE: &str = include_str!("upgrades_generate.sh");
/// Resolve and verify the newest stable Velnor (digest and provenance).
pub(super) const RELEASE: &str = include_str!("upgrades_velnor_release.sh");
const PREPARE: &str = include_str!("upgrades_prepare.sh");
const VERIFY: &str = include_str!("upgrades_verify.sh");
const PUBLISH_CHECK: &str = include_str!("upgrades_publish_check.sh");
/// Push and open or update the pull request (holds the App token).
pub(super) const PUBLISH: &str = include_str!("upgrades_publish.sh");
pub(super) const GATE: &str = include_str!("upgrades_gate.sh");

/// The prepare job's bump script.
pub(super) fn prepare_script() -> String {
    format!("{FILTER}\n{GENERATE}\n{PREPARE}")
}

/// The verify job's regenerate-and-compare script.
pub(super) fn verify_script() -> String {
    format!("{FILTER}\n{GENERATE}\n{VERIFY}")
}

/// The publish job's token-free check of the verified patch.
pub(super) fn publish_check_script() -> String {
    format!("{FILTER}\n{PUBLISH_CHECK}")
}
