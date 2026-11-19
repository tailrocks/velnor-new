// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Access, PermissionScope, Permissions};

use crate::yaml::{Yaml, entry, text};

pub(crate) fn permission_pairs(permissions: &Permissions) -> Vec<(String, String)> {
    permissions
        .0
        .iter()
        .map(|(scope, access)| {
            let scope = match scope {
                PermissionScope::PullRequests => "pull-requests",
                PermissionScope::Actions => "actions",
                PermissionScope::Contents => "contents",
            };
            let access = match access {
                Access::None => "none",
                Access::Read => "read",
                Access::Write => "write",
            };
            (scope.to_owned(), access.to_owned())
        })
        .collect()
}

pub(crate) fn permissions(permissions: &Permissions) -> Yaml {
    Yaml::Map(
        permission_pairs(permissions)
            .iter()
            .map(|(scope, access)| entry(scope, text(access)))
            .collect(),
    )
}
