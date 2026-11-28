// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::identity::{Databases, is_sole_identity, sudo_refused};
use super::parse;
use crate::error::{HostError, RustRunnerFailure};
use crate::rust_runner::RustRunnerLauncher;
use crate::rust_runner::machine::failure;
use crate::rust_runner::shape::{GUEST_USER, GuestUser, USER_HOME};

const NOLOGIN: &str = "/usr/sbin/nologin";
const OWNED_ELSEWHERE: &str = r#"find / -xdev \( -path "$1" -o -path /srv/velnor/gen -o -path /tmp -o -path /var/tmp \) -prune -o \( -user "$2" -o -group "$3" \) -print -quit"#;

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
        if self.has_only_its_own_group(&user)?
            && self.owns_its_ids(&user)?
            && self.owns_nothing_else(&user)?
            && self.cannot_use_sudo()?
        {
            Ok(user)
        } else {
            Err(failure(RustRunnerFailure::GuestUser))
        }
    }

    fn has_only_its_own_group(&self, user: &GuestUser) -> Result<bool, HostError> {
        let numbers = self.read(&["id", "-G", GUEST_USER])?;
        let names = self.read(&["id", "-Gn", GUEST_USER])?;
        Ok(numbers.success()
            && names.success()
            && parse::lines(&numbers.text) == [user.gid().to_string()]
            && parse::lines(&names.text) == [GUEST_USER])
    }

    fn owns_its_ids(&self, user: &GuestUser) -> Result<bool, HostError> {
        let (uid, gid) = (user.uid().to_string(), user.gid().to_string());
        let uid_lookup = self.read(&["getent", "passwd", &uid])?;
        let all_accounts = self.read(&["getent", "passwd"])?;
        let gid_lookup = self.read(&["getent", "group", &gid])?;
        let all_groups = self.read(&["getent", "group"])?;
        let found = Databases {
            uid_lookup: &uid_lookup.text,
            all_accounts: &all_accounts.text,
            gid_lookup: &gid_lookup.text,
            all_groups: &all_groups.text,
        };
        Ok(uid_lookup.success()
            && all_accounts.success()
            && gid_lookup.success()
            && all_groups.success()
            && is_sole_identity(user, &found))
    }

    fn owns_nothing_else(&self, user: &GuestUser) -> Result<bool, HostError> {
        let (uid, gid) = (user.uid().to_string(), user.gid().to_string());
        let found = self.read(&["sh", "-c", OWNED_ELSEWHERE, "sh", USER_HOME, &uid, &gid])?;
        Ok(found.success() && found.text.trim().is_empty())
    }

    fn cannot_use_sudo(&self) -> Result<bool, HostError> {
        let report = self.read(&["sudo", "-n", "-l", "-U", GUEST_USER])?;
        Ok(report.success() && sudo_refused(&report.text))
    }
}
