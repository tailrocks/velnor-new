// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Clone, Copy)]
pub(super) enum PreviewPermissionRole {
    Caller,
    Attest,
    Publish,
}

pub(super) fn preview_permissions(
    role: PreviewPermissionRole,
) -> &'static [(&'static str, &'static str)] {
    match role {
        PreviewPermissionRole::Caller => &[
            ("actions", "read"),
            ("artifact-metadata", "write"),
            ("attestations", "write"),
            ("contents", "write"),
            ("id-token", "write"),
        ],
        PreviewPermissionRole::Attest => &[
            ("actions", "read"),
            ("artifact-metadata", "write"),
            ("attestations", "write"),
            ("contents", "read"),
            ("id-token", "write"),
        ],
        PreviewPermissionRole::Publish => &[("actions", "read"), ("contents", "write")],
    }
}
