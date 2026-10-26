// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::PinnedTool;
use velnor_actions_mise::catalog::rust_channel_manifest_sha256;
use velnor_actions_mise::toolfiles::lockfile::InstallSubject;

pub(super) fn is_catalogued_jactionlint(subject: &InstallSubject) -> bool {
    subject
        .display
        .strip_prefix(PinnedTool::Jactionlint.tool_name())
        .and_then(|rest| rest.strip_prefix('@'))
        .is_some_and(|version| {
            velnor_actions_mise::JACTIONLINT_DIGESTS
                .iter()
                .any(|row| row.version == version)
        })
}

pub(super) fn is_rust(subject: &InstallSubject) -> bool {
    subject
        .display
        .strip_prefix(PinnedTool::Rust.tool_name())
        .is_some_and(|rest| rest.starts_with('@'))
}

pub(super) fn rust_manifest_coverage(subject: &InstallSubject, blocking: &mut Vec<String>) {
    if rust_channel_manifest_sha256(&subject.expected_version).is_some() {
        return;
    }
    let finding = format!(
        "{}:{}; pin a Rust release from the Velnor catalog or upgrade velnor-actions",
        crate::workflow::NOT_CATALOGUED_CODE,
        subject.display
    );
    if !blocking.contains(&finding) {
        blocking.push(finding);
    }
}
