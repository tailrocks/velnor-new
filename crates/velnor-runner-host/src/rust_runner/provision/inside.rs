// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::parse;
use crate::error::{HostError, RustRunnerFailure};
use crate::rust_runner::machine::failure;
use crate::rust_runner::{GUEST_GEN, RustRunnerLauncher};

const MAC_MOUNT: &str = "/mnt/mac";
const ORBSTACK_GUEST: &str = "/opt/orbstack-guest";
const MAC_COMMAND: &str = "/opt/orbstack-guest/bin/mac";
const NO_MAC_MOUNT: &str = r#"test ! -e "$1" && test ! -L "$1""#;
const CAP_QUOTA: &str = r#"options=$(findmnt -n -o OPTIONS -T "$1") || exit 1; id=${options##*subvolid=}; id=${id%%,*}; cat /sys/fs/btrfs/*/qgroups/0_"$id"/max_referenced"#;

pub(super) fn foreign_mounts(table: &str) -> Vec<&str> {
    table
        .lines()
        .filter_map(|line| line.split_once(' '))
        .filter(|(_, kind)| {
            matches!(*kind, "virtiofs" | "9p" | "fuse") || kind.starts_with("fuse.")
        })
        .map(|(target, _)| target)
        .collect()
}

impl RustRunnerLauncher {
    pub(super) fn check_inside(&self) -> Result<(), HostError> {
        self.require_no_mac_mount()?;
        self.require_no_orbstack_sockets()?;
        self.require_mac_command_refused()?;
        self.require_no_mounts()?;
        self.require_cap_quota()
    }

    fn require_no_mac_mount(&self) -> Result<(), HostError> {
        let absent = self.read(&["sh", "-c", NO_MAC_MOUNT, "sh", MAC_MOUNT])?;
        shaped(absent.success())
    }

    fn require_no_orbstack_sockets(&self) -> Result<(), HostError> {
        let found = self.read(&["find", ORBSTACK_GUEST, "-type", "s"])?;
        shaped(found.success() && found.text.trim().is_empty())
    }

    fn require_mac_command_refused(&self) -> Result<(), HostError> {
        let reached = self.read(&[MAC_COMMAND, "uname"])?;
        shaped(!reached.success())
    }

    fn require_no_mounts(&self) -> Result<(), HostError> {
        let table = self.read(&["findmnt", "-rn", "-o", "TARGET,FSTYPE"])?;
        shaped(table.success() && foreign_mounts(&table.text).is_empty())
    }

    fn require_cap_quota(&self) -> Result<(), HostError> {
        let quota = self.read(&["sh", "-c", CAP_QUOTA, "sh", GUEST_GEN])?;
        let limits = parse::lines(&quota.text);
        shaped(
            quota.success()
                && limits.len() == 1
                && limits[0].parse::<u64>().ok() == Some(self.disk_cap_bytes()),
        )
    }
}

fn shaped(holds: bool) -> Result<(), HostError> {
    if holds {
        Ok(())
    } else {
        Err(failure(RustRunnerFailure::MachineShape))
    }
}
