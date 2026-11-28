// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::parse;
use crate::error::{HostError, RustRunnerFailure};
use crate::rust_runner::machine::failure;
use crate::rust_runner::{GUEST_GEN, GUEST_SRV, GUEST_STORE, RustRunnerLauncher};

const MAC_MOUNT: &str = "/mnt/mac";
const ORBSTACK_GUEST: &str = "/opt/orbstack-guest";
const MAC_COMMAND: &str = "/opt/orbstack-guest/bin/mac";
const MAC_REFUSED: &str = "velnor-mac-refused";
const MAC_REFUSAL: &str =
    r#"if "$1" uname >/dev/null 2>&1; then exit 1; fi; echo velnor-mac-refused"#;
const NO_MAC_MOUNT: &str = r#"test ! -e "$1" && test ! -L "$1""#;
const CAP_QUOTA: &str = r#"options=$(findmnt -n -o OPTIONS -T "$1") || exit 1; id=${options##*subvolid=}; id=${id%%,*}; case "$id" in ''|*[!0-9]*) exit 1;; esac; for path in "$@"; do test "$(btrfs inspect-internal rootid "$path")" = "$id" || exit 2; done; cat /sys/fs/btrfs/*/qgroups/0_"$id"/max_referenced"#;
const CAPPED_PATHS: [&str; 5] = [
    GUEST_STORE,
    GUEST_SRV,
    GUEST_GEN,
    "/srv/velnor/gen/work",
    "/srv/velnor/gen/stage",
];
const LAYOUT_ROOTS: [&str; 2] = [GUEST_STORE, GUEST_SRV];
const JOB_PARENTS: [&str; 2] = ["/srv/velnor/gen/work/", "/srv/velnor/gen/stage/"];

fn under(target: &str, root: &str) -> bool {
    target == root
        || target
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn foreign(kind: &str) -> bool {
    matches!(kind, "virtiofs" | "9p" | "fuse") || kind.starts_with("fuse.")
}

pub(super) fn refused_mounts(table: &str) -> Vec<&str> {
    table
        .lines()
        .filter_map(|line| line.split_once(' '))
        .filter(|(target, kind)| {
            foreign(kind)
                || (LAYOUT_ROOTS.iter().any(|root| under(target, root))
                    && !JOB_PARENTS.iter().any(|parent| target.starts_with(parent)))
        })
        .map(|(target, _)| target)
        .collect()
}

impl RustRunnerLauncher {
    pub(super) fn check_inside(&self) -> Result<(), HostError> {
        self.require_no_mac_mount()?;
        self.require_no_orbstack_sockets()?;
        self.require_mac_command_refused()?;
        self.require_no_setuid_fuse()?;
        self.require_cap_quota()
    }

    fn require_no_mac_mount(&self) -> Result<(), HostError> {
        let absent = self.read(&["sh", "-c", NO_MAC_MOUNT, "sh", MAC_MOUNT])?;
        shaped(absent.success())
    }

    fn require_no_orbstack_sockets(&self) -> Result<(), HostError> {
        let found = self.read(&["find", "-H", ORBSTACK_GUEST, "-type", "s"])?;
        shaped(found.success() && found.text.trim().is_empty())
    }

    fn require_mac_command_refused(&self) -> Result<(), HostError> {
        let reached = self.read(&["sh", "-c", MAC_REFUSAL, "sh", MAC_COMMAND])?;
        shaped(reached.success() && parse::lines(&reached.text) == [MAC_REFUSED])
    }

    pub(super) fn require_no_mounts(&self) -> Result<(), HostError> {
        let table = self.read(&["findmnt", "-rn", "-o", "TARGET,FSTYPE"])?;
        shaped(table.success() && refused_mounts(&table.text).is_empty())
    }

    fn require_cap_quota(&self) -> Result<(), HostError> {
        let mut args = vec!["sh", "-c", CAP_QUOTA, "sh", GUEST_GEN];
        args.extend(CAPPED_PATHS);
        let quota = self.read(&args)?;
        let limits = parse::lines(&quota.text);
        shaped(
            quota.success()
                && limits.len() == 1
                && limits[0].parse::<u64>().ok() == Some(self.disk_cap_bytes()),
        )
    }
}

pub(super) fn shaped(holds: bool) -> Result<(), HostError> {
    if holds {
        Ok(())
    } else {
        Err(failure(RustRunnerFailure::MachineShape))
    }
}
