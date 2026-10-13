//! Fixed Git controls belong to the discovery constructor.
//! Optional locks prevent status refresh writes; Git diff needs separate protection.

use std::ffi::OsString;

use super::{EnvPolicy, IsolatedCommand};

impl IsolatedCommand {
    pub(crate) fn direct(program: &str, args: Vec<OsString>) -> Self {
        let extra_env = if program == "git" {
            vec![(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("0"))]
        } else {
            Vec::new()
        };
        Self {
            program: OsString::from(program),
            args,
            cwd: None,
            extra_env,
            policy: EnvPolicy::Discovery,
        }
    }
}
