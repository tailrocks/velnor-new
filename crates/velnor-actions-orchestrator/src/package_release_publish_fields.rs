//! Literal-only JSON mutation fields for the native publisher transport.

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
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return fixture.mutate(method, route, fields);
        }
        let mut args = vec![
            OsString::from(route),
            OsString::from("--method"),
            OsString::from(method),
        ];
        for (key, value) in fields {
            // `make_latest` is a string enum; draft, prerelease and force are booleans.
            // Raw fields also prevent accidental @file expansion of literal data.
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
