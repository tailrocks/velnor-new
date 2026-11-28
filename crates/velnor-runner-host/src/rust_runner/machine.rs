// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use super::guest_user::GuestUser;
use super::store::{HostHold, Store};
use super::{
    GUEST_RUNNER, GUEST_STORE, MACHINE_NAME, ORB_CREATE_DEADLINE, RustRunnerLauncher, orb,
};
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
}

impl RustRunnerLauncher {
    pub fn ensure_machine(&self) -> Result<GuestUser, HostError> {
        if !self.machine_exists()? {
            self.create_machine()?;
        }
        Store::prepare(&self.host_store)?;
        self.link_store()?;
        self.require_runner_binary()?;
        self.guest_user()
    }

    pub fn hold_host(&self) -> Result<HostHold, HostError> {
        Store::create(&self.host_store)?.hold()
    }

    pub fn runner_digest(&self) -> Result<String, HostError> {
        Store::open(&self.host_store)?.runner_digest()
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
            Some(image) if image.distro == MACHINE_DISTRO && image.arch == MACHINE_ARCH => Ok(true),
            _ => Err(failure(RustRunnerFailure::MachineIncompatible)),
        }
    }

    fn create_machine(&self) -> Result<(), HostError> {
        let mut command = self.orb();
        command.args(["create", "-a", "arm64", "ubuntu", MACHINE_NAME]);
        succeeds(
            command,
            self.deadline.max(ORB_CREATE_DEADLINE),
            RustRunnerFailure::MachineCreate,
        )
    }

    fn link_store(&self) -> Result<(), HostError> {
        let target = format!("/mnt/mac{}", self.host_store.display());
        let mut command = self.as_root();
        command.args(["ln", "-sfnT", &target, GUEST_STORE]);
        succeeds(command, self.deadline, RustRunnerFailure::MachineLink)
    }

    fn require_runner_binary(&self) -> Result<(), HostError> {
        let mut command = self.in_machine();
        command.args(["test", "-x", GUEST_RUNNER]);
        succeeds(command, self.deadline, RustRunnerFailure::BinaryMissing)
    }
}

fn succeeds(
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
