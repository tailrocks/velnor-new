//! Check Rust install audit coverage through the channel manifest digest.

use velnor_actions_mise::PinnedTool;
use velnor_actions_mise::catalog::rust_channel_manifest_sha256;
use velnor_actions_mise::toolfiles::lockfile::InstallSubject;

/// Whether an audit subject is the `core:rust` toolchain.
/// A `jactionlint` install whose version has catalog executable digests.
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

/// A job with `Verify Rust channel manifest` checks Rust manifest metadata against the catalog digest.
/// It uses this check because `core:rust` records no checksum in `mise.lock`.
/// Generation fails if the catalog has no digest for the version.
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

#[cfg(test)]
#[path = "lock_audit_rust_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lock_audit_main_rust_tests.rs"]
mod main_tests;
