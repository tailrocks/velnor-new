// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::parse;
use crate::error::{HostError, RustRunnerFailure};
use crate::rust_runner::RustRunnerLauncher;
use crate::rust_runner::machine::failure;
use crate::rust_runner::shape::{GUEST_USER, GuestUser};

const NOLOGIN: &str = "/usr/sbin/nologin";
const USER_HOME: &str = "/home/velnor-job";

impl RustRunnerLauncher {
    pub(super) fn job_user(&self) -> Result<GuestUser, HostError> {
        if self.read(&["getent", "passwd", GUEST_USER])?.code == Some(2) {
            self.apply(&[
                "useradd",
                "--system",
                "--user-group",
                "--no-create-home",
                "--home-dir",
                USER_HOME,
                "--shell",
                NOLOGIN,
                GUEST_USER,
            ])?;
        }
        let passwd = self.read(&["getent", "passwd", GUEST_USER])?;
        let user = GuestUser::parse(&passwd.text)
            .filter(|_| passwd.success())
            .ok_or_else(|| failure(RustRunnerFailure::GuestUser))?;
        let numbers = self.read(&["id", "-G", GUEST_USER])?;
        let names = self.read(&["id", "-Gn", GUEST_USER])?;
        let own_group = numbers.success()
            && names.success()
            && parse::lines(&numbers.text) == [user.gid().to_string()]
            && parse::lines(&names.text) == [GUEST_USER];
        let sudoers = self.read(&[
            "grep",
            "-rsEw",
            "--",
            GUEST_USER,
            "/etc/sudoers",
            "/etc/sudoers.d",
        ])?;
        if own_group && sudoers.code == Some(1) {
            Ok(user)
        } else {
            Err(failure(RustRunnerFailure::GuestUser))
        }
    }
}
