pub(super) const FILTER: &str = include_str!("upgrades_patch_filter.sh");
const GENERATE: &str = include_str!("upgrades_generate.sh");
pub(super) const RELEASE: &str = include_str!("upgrades_velnor_release.sh");
const PREPARE: &str = include_str!("upgrades_prepare.sh");
const VERIFY: &str = include_str!("upgrades_verify.sh");
const PUBLISH_CHECK: &str = include_str!("upgrades_publish_check.sh");
pub(super) const PUBLISH: &str = include_str!("upgrades_publish.sh");
pub(super) const GATE: &str = include_str!("upgrades_gate.sh");

pub(super) fn prepare_script() -> String {
    format!("{FILTER}\n{GENERATE}\n{PREPARE}")
}

pub(super) fn verify_script() -> String {
    format!("{FILTER}\n{GENERATE}\n{VERIFY}")
}

pub(super) fn publish_check_script() -> String {
    format!("{FILTER}\n{PUBLISH_CHECK}")
}
