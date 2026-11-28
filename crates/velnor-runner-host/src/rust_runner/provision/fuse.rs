// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::inside::shaped;
use crate::error::HostError;
use crate::rust_runner::RustRunnerLauncher;

const BINARY_DIRECTORIES: [&str; 6] = [
    "/usr/bin",
    "/usr/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/bin",
    "/sbin",
];
const SETUID_FUSE: &str = r#"find -H "$@" -maxdepth 1 -type f \( -name fusermount -o -name fusermount3 \) \( -perm -4000 -o -perm -2000 \)"#;

impl RustRunnerLauncher {
    pub(super) fn strip_fuse_setuid(&self) -> Result<(), HostError> {
        let script = format!("{SETUID_FUSE} -exec chmod u-s,g-s {{}} +");
        self.read(&fuse_args(&script))?;
        Ok(())
    }

    pub(super) fn require_no_setuid_fuse(&self) -> Result<(), HostError> {
        let found = self.read(&fuse_args(SETUID_FUSE))?;
        shaped(found.success() && found.text.trim().is_empty())
    }
}

fn fuse_args(script: &str) -> Vec<&str> {
    let mut args = vec!["sh", "-c", script, "sh"];
    args.extend(BINARY_DIRECTORIES);
    args
}
