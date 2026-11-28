// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use super::shape::{GuestUser, ListedConfig};
use super::store::{HostHold, Store};
use super::{GIB, MACHINE_NAME, ORB_CREATE_DEADLINE, RustRunnerLauncher, orb, runner_binary};
use crate::error::{HostError, RustRunnerFailure};

const MACHINE_DISTRO: &str = "ubuntu";
const MACHINE_ARCH: &str = "arm64";

#[derive(Deserialize)]
struct ListedImage {
    distro: String,
    arch: String,
}

#[derive(Deserialize)]
struct ListedMachine {
    name: String,
    image: Option<ListedImage>,
    config: Option<ListedConfig>,
}

impl RustRunnerLauncher {
    pub fn ensure_machine(&self) -> Result<GuestUser, HostError> {
        if !self.machine_exists()? {
            Store::create(&self.host_store)?;
            self.create_machine()?;
            if !self.machine_exists()? {
                return Err(failure(RustRunnerFailure::MachineCreate));
            }
        }
        Store::create(&self.host_store)?.require_no_acl()?;
        self.provision(&self.runner_digest()?)
    }

    pub fn hold_host(&self) -> Result<HostHold, HostError> {
        Store::create(&self.host_store)?.hold()
    }

    pub fn runner_digest(&self) -> Result<String, HostError> {
        runner_binary::digest(&self.runner_binary)
    }

    pub(super) const fn disk_cap_bytes(&self) -> u64 {
        self.disk_cap_gib.saturating_mul(GIB)
    }

    fn machine_exists(&self) -> Result<bool, HostError> {
        let mut command = self.orb();
        command.args(["list", "-f", "json"]);
        let output = orb::run(command, self.deadline, true).map_err(failure)?;
        if !output.status.success() {
            return Err(failure(RustRunnerFailure::MachineList));
        }
        let machines: Vec<ListedMachine> = serde_json::from_slice(&output.stdout)
            .map_err(|_| failure(RustRunnerFailure::MachineList))?;
        let Some(machine) = machines.iter().find(|machine| machine.name == MACHINE_NAME) else {
            return Ok(false);
        };
        match &machine.image {
            Some(image) if image.distro == MACHINE_DISTRO && image.arch == MACHINE_ARCH => {}
            _ => return Err(failure(RustRunnerFailure::MachineIncompatible)),
        }
        match &machine.config {
            Some(config) if config.matches(self.disk_cap_bytes()) => Ok(true),
            _ => Err(failure(RustRunnerFailure::MachineShape)),
        }
    }

    fn create_machine(&self) -> Result<(), HostError> {
        let disk = format!("{}G", self.disk_cap_gib);
        let mut command = self.orb();
        command.args([
            "create",
            "--isolated",
            "--isolate-network",
            "--disk",
            &disk,
            "-a",
            MACHINE_ARCH,
            MACHINE_DISTRO,
            MACHINE_NAME,
        ]);
        succeeds(
            command,
            self.deadline.max(ORB_CREATE_DEADLINE),
            RustRunnerFailure::MachineCreate,
        )
    }
}

pub(super) fn succeeds(
    command: Command,
    deadline: Duration,
    otherwise: RustRunnerFailure,
) -> Result<(), HostError> {
    let finished = orb::run(command, deadline, false).map_err(failure)?;
    if finished.status.success() {
        Ok(())
    } else {
        Err(failure(otherwise))
    }
}

pub(super) const fn failure(cause: RustRunnerFailure) -> HostError {
    HostError::RustRunner(cause)
}
