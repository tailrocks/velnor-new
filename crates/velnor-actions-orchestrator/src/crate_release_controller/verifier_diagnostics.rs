// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn report_verifier_failure(
    stderr: &[u8],
    code: Option<i32>,
    signal: Option<i32>,
    deadline_expired: bool,
) {
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
        "capacity verifier failure category: {category}; code={code:?}; signal={signal:?}; deadline_expired={deadline_expired}"
    );
}
