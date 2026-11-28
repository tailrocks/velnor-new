// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use self::parse::Entry;
use super::machine::{failure, succeeds};
use super::shape::GuestUser;
use super::{GUEST_GEN, GUEST_SRV, GUEST_STORE, RustRunnerLauncher, orb};
use crate::error::{ErrorDetail, HostError, RustRunnerFailure};

mod cap;
mod fuse;
mod identity;
mod inside;
mod job_dirs;
mod parse;
mod runner_copy;
#[cfg(test)]
mod shell_tests;
#[cfg(test)]
mod tests;
mod user;

const ANCESTORS: [&str; 4] = ["/", "/var", "/var/lib", "/srv"];
const OPEN_DIRECTORIES: [&str; 10] = [
    GUEST_STORE,
    "/var/lib/velnor/bin",
    "/var/lib/velnor/tools",
    "/var/lib/velnor/crates",
    "/var/lib/velnor/cache",
    "/var/lib/velnor/git",
    GUEST_SRV,
    GUEST_GEN,
    "/srv/velnor/gen/work",
    "/srv/velnor/gen/stage",
];
const CLOSED_DIRECTORIES: [&str; 4] = [
    "/var/lib/velnor/locks",
    "/var/lib/velnor/aside",
    "/var/lib/velnor/incoming",
    "/var/lib/velnor/live",
];
const STORE_FILESYSTEM: &str = "btrfs";
const PROTECTED_LINKS: [&str; 2] = ["fs.protected_hardlinks", "fs.protected_symlinks"];
const TOOL_PACKAGES: [&str; 2] = ["nftables", "btrfs-progs"];
const REQUIRED_TOOLS: [&str; 3] = ["nft", "ip", "btrfs"];
const TOOLS_PROBE: &str = r#"for tool in "$@"; do command -v "$tool" >/dev/null || exit 1; done"#;
const DIRECTORY: &str = "directory";
const REGULAR_FILE: &str = "regular file";

struct Output {
    code: Option<i32>,
    text: String,
    stderr: String,
}

impl Output {
    fn success(&self) -> bool {
        self.code == Some(0)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Layout {
    Sound,
    Repair,
    Unsafe,
    Foreign(String),
}

fn classify(entries: &HashMap<String, Entry>) -> Layout {
    let closed_ancestors = ANCESTORS.iter().all(|path| {
        entries
            .get(*path)
            .is_some_and(Entry::is_closed_root_directory)
    });
    if !closed_ancestors {
        return Layout::Unsafe;
    }
    let stored = || OPEN_DIRECTORIES.iter().chain(&CLOSED_DIRECTORIES);
    if stored().any(|path| {
        entries
            .get(*path)
            .is_some_and(|entry| entry.kind != DIRECTORY)
    }) {
        return Layout::Unsafe;
    }
    let foreign = stored().find(|path| entries.get(**path).is_some_and(|entry| entry.uid != 0));
    if let Some(path) = foreign {
        return Layout::Foreign((*path).to_owned());
    }
    let sound = |paths: &[&str], mode| {
        paths.iter().all(|path| {
            entries
                .get(*path)
                .is_some_and(|entry| entry.is(DIRECTORY, mode))
        })
    };
    if sound(&OPEN_DIRECTORIES, 0o755) && sound(&CLOSED_DIRECTORIES, 0o700) {
        Layout::Sound
    } else {
        Layout::Repair
    }
}

impl RustRunnerLauncher {
    pub(super) fn provision(&self, runner_digest: &str) -> Result<GuestUser, HostError> {
        self.require_no_mounts()?;
        self.ensure_layout()?;
        self.require_btrfs()?;
        self.require_protected_links()?;
        self.ensure_tools()?;
        self.strip_fuse_setuid()?;
        self.check_inside()?;
        let user = self.job_user()?;
        self.ensure_cap()?;
        self.refresh_runner(runner_digest)?;
        Ok(user)
    }

    fn read(&self, args: &[&str]) -> Result<Output, HostError> {
        let mut command = self.guest();
        command.args(args);
        let finished = orb::run(command, self.deadline, true).map_err(failure)?;
        Ok(Output {
            code: finished.status.code(),
            text: String::from_utf8_lossy(&finished.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&finished.stderr).into_owned(),
        })
    }

    fn apply(&self, args: &[&str]) -> Result<(), HostError> {
        let mut command = self.guest();
        command.args(args);
        succeeds(command, self.deadline, RustRunnerFailure::MachineProvision)
    }

    fn stat(&self, paths: &[&str]) -> Result<HashMap<String, Entry>, HostError> {
        let mut args = vec!["stat", "-c", "%n %u %g %a %F"];
        args.extend_from_slice(paths);
        Ok(parse::entries(&self.read(&args)?.text))
    }

    fn layout(&self) -> Result<Layout, HostError> {
        let mut paths = ANCESTORS.to_vec();
        paths.extend(OPEN_DIRECTORIES);
        paths.extend(CLOSED_DIRECTORIES);
        Ok(classify(&self.stat(&paths)?))
    }

    fn ensure_layout(&self) -> Result<(), HostError> {
        match self.layout()? {
            Layout::Sound => return Ok(()),
            Layout::Unsafe => return Err(failure(RustRunnerFailure::MachineLayout)),
            Layout::Foreign(path) => {
                return Err(failure(RustRunnerFailure::StoreOwner(
                    ErrorDetail::from_bytes(path.as_bytes()),
                )));
            }
            Layout::Repair => {}
        }
        for (mode, directories) in [
            ("0755", &OPEN_DIRECTORIES[..]),
            ("0700", &CLOSED_DIRECTORIES),
        ] {
            let mut args = vec!["install", "-d", "-o", "root", "-g", "root", "-m", mode];
            args.extend_from_slice(directories);
            self.apply(&args)?;
        }
        if self.layout()? == Layout::Sound {
            Ok(())
        } else {
            Err(failure(RustRunnerFailure::MachineLayout))
        }
    }

    fn require_btrfs(&self) -> Result<(), HostError> {
        let found = self.read(&["stat", "-f", "-c", "%T", GUEST_GEN, GUEST_STORE])?;
        let types = parse::lines(&found.text);
        if found.success() && types.len() == 2 && types.iter().all(|kind| *kind == STORE_FILESYSTEM)
        {
            Ok(())
        } else {
            Err(failure(RustRunnerFailure::MachineFilesystem))
        }
    }

    fn require_protected_links(&self) -> Result<(), HostError> {
        let mut args = vec!["sysctl", "-n"];
        args.extend(PROTECTED_LINKS);
        let found = self.read(&args)?;
        let values = parse::lines(&found.text);
        if found.success()
            && values.len() == PROTECTED_LINKS.len()
            && values.iter().all(|v| *v == "1")
        {
            Ok(())
        } else {
            Err(failure(RustRunnerFailure::MachineShape))
        }
    }

    fn tools_present(&self) -> Result<bool, HostError> {
        let mut args = vec!["sh", "-c", TOOLS_PROBE, "sh"];
        args.extend(REQUIRED_TOOLS);
        Ok(self.read(&args)?.success())
    }

    fn ensure_tools(&self) -> Result<(), HostError> {
        if self.tools_present()? {
            return Ok(());
        }
        let mut args = vec![
            "env",
            "DEBIAN_FRONTEND=noninteractive",
            "apt-get",
            "install",
            "-y",
            "--no-install-recommends",
        ];
        args.extend(TOOL_PACKAGES);
        self.read(&args)?;
        if self.tools_present()? {
            Ok(())
        } else {
            Err(failure(RustRunnerFailure::MachineTools))
        }
    }
}
