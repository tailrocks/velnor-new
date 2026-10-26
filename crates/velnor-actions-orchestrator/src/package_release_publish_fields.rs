// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GitHubApi, JSON_LIMIT};
use crate::OrchestratorError;
use std::ffi::OsString;

impl GitHubApi {
    pub(super) fn fields_request(
        &self,
        route: &str,
        method: &str,
        fields: &[(&str, String)],
    ) -> Result<Vec<u8>, OrchestratorError> {
        let mut args = vec![
            OsString::from(route),
            OsString::from("--method"),
            OsString::from(method),
        ];
        for (key, value) in fields {
            args.push(OsString::from(
                if matches!(*key, "draft" | "prerelease" | "force") {
                    "--field"
                } else {
                    "--raw-field"
                },
            ));
            args.push(OsString::from(format!("{key}={value}")));
        }
        self.request(args, JSON_LIMIT)
    }
}
