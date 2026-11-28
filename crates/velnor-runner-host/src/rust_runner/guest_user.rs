// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::machine::failure;
use super::{RustRunnerLauncher, orb};
use crate::error::{HostError, RustRunnerFailure};

const HOME_ROOT: &str = "/home";
const PROBE: &str = r#"echo "$(id -u) $(id -g) $HOME""#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestUser {
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) home: String,
}

fn parse(output: &str) -> Option<GuestUser> {
    let mut fields = output.split_whitespace();
    let user = GuestUser {
        uid: fields.next()?.parse().ok()?,
        gid: fields.next()?.parse().ok()?,
        home: fields.next()?.to_owned(),
    };
    (fields.next().is_none() && usable(&user)).then_some(user)
}

fn usable(user: &GuestUser) -> bool {
    user.uid != 0
        && user.gid != 0
        && user
            .home
            .strip_prefix(HOME_ROOT)
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|name| !name.is_empty() && !name.contains("..") && !name.ends_with('/'))
}

impl RustRunnerLauncher {
    pub(super) fn guest_user(&self) -> Result<GuestUser, HostError> {
        let mut command = self.in_machine();
        command.args(["sh", "-c", PROBE]);
        let output = orb::run(command, self.deadline, true).map_err(failure)?;
        String::from_utf8(output.stdout)
            .ok()
            .filter(|_| output.status.success())
            .and_then(|text| parse(&text))
            .ok_or_else(|| failure(RustRunnerFailure::GuestUser))
    }
}
