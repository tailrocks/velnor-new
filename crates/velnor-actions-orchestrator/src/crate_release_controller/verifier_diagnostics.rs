// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn report_verifier_failure(
    stdout: &[u8],
    stderr: &[u8],
    code: Option<i32>,
    signal: Option<i32>,
    deadline_expired: bool,
) {
    let reason = pin_reason(stdout);
    let text = String::from_utf8_lossy(stderr);
    let category = [
        "rust_tree_digest_mismatch",
        "rust_installed_manifest_unwritable",
        "rust_installed_manifest_missing",
        "rust_installed_manifest_empty",
        "rust_toolchain_path_unexpected",
        "rust_toolchain_file_unexpected",
        "rust_toolchain_missing",
        "rust_component_url_untrusted",
        "rust_component_name_invalid",
        "Permission denied",
        "No such file or directory",
        "File exists",
        "unbound variable",
        "parameter not set",
        "rust_component_tarball_digest_mismatch",
        "rust_channel_manifest_digest_mismatch",
        "rust_component_not_in_manifest",
        "rust_toolchain_bytes_mismatch",
        "rust_component_archive_invalid",
        "curl: (",
    ]
    .into_iter()
    .find(|candidate| text.contains(candidate))
    .unwrap_or("other");
    eprintln!(
        "capacity verifier failure category: {category}; pin_reason={reason}; code={code:?}; signal={signal:?}; deadline_expired={deadline_expired}"
    );
}

fn pin_reason(stdout: &[u8]) -> &'static str {
    let text = String::from_utf8_lossy(stdout);
    text.lines()
        .filter_map(|line| line.strip_prefix("::notice::pin_mismatch: "))
        .find_map(|reason| match reason {
            "tree_differs" => Some("tree_differs"),
            "tree_unreadable" => Some("tree_unreadable"),
            "unsafe_name" => Some("unsafe_name"),
            "listing_failed" => Some("listing_failed"),
            "toolchains_differ" => Some("toolchains_differ"),
            reason if reason.starts_with("components_not_pinned ") => Some("components_not_pinned"),
            reason if reason.starts_with("no_pin ") => Some("no_pin"),
            _ => None,
        })
        .unwrap_or("unavailable")
}

#[cfg(test)]
mod tests {
    use super::pin_reason;

    #[test]
    fn pin_reason_only_returns_known_fixed_codes() {
        assert_eq!(
            pin_reason(b"::notice::pin_mismatch: tree_differs\n"),
            "tree_differs"
        );
        assert_eq!(
            pin_reason(b"::notice::pin_mismatch: components_not_pinned components=private\n"),
            "components_not_pinned"
        );
        assert_eq!(
            pin_reason(b"::notice::pin_mismatch: tree_differs secret\n"),
            "unavailable"
        );
        assert_eq!(
            pin_reason(b"secret\n::notice::pin_mismatch: secret\n"),
            "unavailable"
        );
    }
}
