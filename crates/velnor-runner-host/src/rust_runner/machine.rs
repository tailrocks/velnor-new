// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::DirBuilder;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::process::{Command, Stdio};

use serde::Deserialize;

use super::{
    GUEST_RUNNER, GUEST_STORE, HOST_DIRECTORIES, JIT_DIRECTORY, MAC_MOUNT, MACHINE_ARCH,
    MACHINE_DISTRO, MACHINE_NAME, RustRunnerLauncher,
};
use crate::error::{HostError, RustRunnerFailure};

#[derive(Deserialize)]
struct ListedMachine {
    name: String,
}

impl RustRunnerLauncher {
    pub fn ensure_machine(&self) -> Result<(), HostError> {
        if !self.machine_exists()? {
            self.create_machine()?;
        }
        self.create_host_directories()?;
        self.link_store()?;
        self.require_runner_binary()
    }

    fn machine_exists(&self) -> Result<bool, HostError> {
        let output = self
            .orb()
            .args(["list", "-f", "json"])
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .map_err(|_| failure(RustRunnerFailure::OrbUnavailable))?;
        if !output.status.success() {
            return Err(failure(RustRunnerFailure::MachineList));
        }
        let machines: Vec<ListedMachine> = serde_json::from_slice(&output.stdout)
            .map_err(|_| failure(RustRunnerFailure::MachineList))?;
        Ok(machines.iter().any(|machine| machine.name == MACHINE_NAME))
    }

    fn create_machine(&self) -> Result<(), HostError> {
        let mut command = self.orb();
        command.args(["create", "-a", MACHINE_ARCH, MACHINE_DISTRO, MACHINE_NAME]);
        succeeds(&mut command, RustRunnerFailure::MachineCreate)
    }

    fn create_host_directories(&self) -> Result<(), HostError> {
        for name in HOST_DIRECTORIES {
            std::fs::create_dir_all(self.host_store.join(name))
                .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        }
        self.create_private_jit_directory()
    }

    fn create_private_jit_directory(&self) -> Result<(), HostError> {
        let directory = self.host_store.join(JIT_DIRECTORY);
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)
            .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        let metadata = std::fs::symlink_metadata(&directory)
            .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        if !metadata.is_dir() {
            return Err(failure(RustRunnerFailure::HostStore));
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(failure(RustRunnerFailure::JitDirectoryAccessible));
        }
        Ok(())
    }

    fn link_store(&self) -> Result<(), HostError> {
        let target = format!("{MAC_MOUNT}{}", self.host_store.display());
        let mut command = self.as_root();
        command.args(["ln", "-sfn", &target, GUEST_STORE]);
        succeeds(&mut command, RustRunnerFailure::MachineLink)
    }

    fn require_runner_binary(&self) -> Result<(), HostError> {
        let mut command = self.in_machine();
        command.args(["test", "-x", GUEST_RUNNER]);
        succeeds(&mut command, RustRunnerFailure::BinaryMissing)
    }
}

fn succeeds(command: &mut Command, otherwise: RustRunnerFailure) -> Result<(), HostError> {
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|_| failure(RustRunnerFailure::OrbUnavailable))?;
    if status.success() {
        Ok(())
    } else {
        Err(failure(otherwise))
    }
}

pub(super) const fn failure(cause: RustRunnerFailure) -> HostError {
    HostError::RustRunner(cause)
}
