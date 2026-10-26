// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GitHubApi, OrchestratorError, internal};
use std::{ffi::OsString, path::Path};

impl GitHubApi {
    pub(super) fn upload_arguments(
        &self,
        route: &str,
        path: &Path,
    ) -> Result<Vec<OsString>, OrchestratorError> {
        let suffix = route
            .strip_prefix(&self.route("releases/"))
            .ok_or_else(|| internal("package_upload_repository_route"))?;
        let (id, name) = suffix
            .split_once("/assets?name=")
            .ok_or_else(|| internal("package_upload_route"))?;
        if id
            .parse::<u64>()
            .ok()
            .is_none_or(|number| number == 0 || number.to_string() != id)
            || name.is_empty()
            || name.len() > 255
            || matches!(name, "." | "..")
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(internal("package_upload_identity"));
        }
        Ok(vec![
            OsString::from(format!("https://uploads.github.com/{route}")),
            OsString::from("--method"),
            OsString::from("POST"),
            OsString::from("--header"),
            OsString::from("Content-Type: application/octet-stream"),
            OsString::from("--input"),
            path.as_os_str().to_owned(),
        ])
    }
}
