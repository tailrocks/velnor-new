// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const FILTER: &str = include_str!("upgrades_patch_filter.sh");
const GENERATE: &str = include_str!("upgrades_generate.sh");
const RELEASE: &str = include_str!("upgrades_velnor_release.sh");
const PREPARE: &str = include_str!("upgrades_prepare.sh");
const VERIFY: &str = include_str!("upgrades_verify.sh");
const PUBLISH_CHECK: &str = include_str!("upgrades_publish_check.sh");
const PUBLISH: &str = include_str!("upgrades_publish.sh");
const GATE: &str = include_str!("upgrades_gate.sh");

pub(super) fn prepare_script() -> String {
    format!("{FILTER}\n{GENERATE}\n{PREPARE}")
}

pub(super) fn verify_script() -> String {
    format!("{FILTER}\n{GENERATE}\n{VERIFY}")
}

pub(super) fn publish_check_script() -> String {
    format!("{FILTER}\n{PUBLISH_CHECK}")
}

fn with_verified_gh(script: &str) -> String {
    let pin = velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64;
    let command = crate::verified_gh_shell::command();
    let function = format!(
        "velnor_gh() {{ [ \"${{VELNOR_GH_VERSION:-}}\" = '{}' ] || return 1; {command} \"$@\"; }}",
        pin.version
    );
    script.replace("@VERIFIED_GH_FUNCTION@", &function)
}

pub(super) fn release_script() -> String {
    with_verified_gh(RELEASE)
}
pub(super) fn publish_script() -> String {
    with_verified_gh(PUBLISH)
}
pub(super) fn gate_script() -> String {
    with_verified_gh(GATE)
}
