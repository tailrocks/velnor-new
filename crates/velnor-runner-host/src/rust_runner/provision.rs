// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use self::parse::Entry;
use super::machine::{failure, succeeds};
use super::shape::GuestUser;
use super::{
    GIB, GUEST_CAP_RECORD, GUEST_GEN, GUEST_RUNNER, GUEST_SHARE, GUEST_SRV, GUEST_STORE,
    RustRunnerLauncher, orb, shape,
};
use crate::error::{HostError, RustRunnerFailure};

mod job_dirs;
mod parse;
#[cfg(test)]
mod tests;

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
const RUNNER_STAGING: &str = "/var/lib/velnor/bin/.velnor-runner.new";
const CAP_PROBE_FILE: &str = "/var/lib/velnor/.cap-probe";
const STORE_FILESYSTEM: &str = "btrfs";
const PROTECTED_LINKS: [&str; 2] = ["fs.protected_hardlinks", "fs.protected_symlinks"];
const USER_PROBE: &str = r#"printf '%s\n%s\n%s\n' "$(id -u)" "$(id -g)" "$HOME""#;
const RECORD_WRITER: &str = r#"printf '%s\n' "$1" > "$2.new" && mv -f "$2.new" "$2""#;
const DIRECTORY: &str = "directory";
const REGULAR_FILE: &str = "regular file";

struct Output {
    code: Option<i32>,
    text: String,
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
    pub(super) fn provision(&self, runner_digest: &str) -> Result<(), HostError> {
        self.ensure_layout()?;
        self.require_btrfs()?;
        self.require_protected_links()?;
        self.ensure_cap()?;
        self.refresh_runner(runner_digest)
    }

    fn read(&self, args: &[&str]) -> Result<Output, HostError> {
        let mut command = self.as_root();
        command.args(args);
        let finished = orb::run(command, self.deadline, true).map_err(failure)?;
        Ok(Output {
            code: finished.status.code(),
            text: String::from_utf8_lossy(&finished.stdout).into_owned(),
        })
    }

    fn apply(&self, args: &[&str]) -> Result<(), HostError> {
        let mut command = self.as_root();
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

    pub(super) fn guest_user(&self) -> Result<GuestUser, HostError> {
        let mut command = self.in_machine();
        command.args(["sh", "-c", USER_PROBE]);
        let output = orb::run(command, self.deadline, true).map_err(failure)?;
        GuestUser::parse(&String::from_utf8_lossy(&output.stdout))
            .filter(|_| output.status.success())
            .ok_or_else(|| failure(RustRunnerFailure::GuestUser))
    }

    fn cap_record(&self) -> Result<String, HostError> {
        let mut command = self.orb();
        command.arg("version");
        let output = orb::run(command, self.deadline, true).map_err(failure)?;
        let version = shape::orb_version(&String::from_utf8_lossy(&output.stdout))
            .filter(|_| output.status.success())
            .ok_or_else(|| failure(RustRunnerFailure::MachineProvision))?;
        Ok(format!("orbstack={version} cap={}", self.disk_cap_bytes()))
    }

    fn ensure_cap(&self) -> Result<(), HostError> {
        let record = self.cap_record()?;
        let recorded = self.read(&["cat", GUEST_CAP_RECORD])?;
        if recorded.success() && recorded.text.trim() == record {
            return Ok(());
        }
        let over_cap = self.disk_cap_bytes().saturating_add(GIB).to_string();
        let attempt = self.read(&["fallocate", "-l", &over_cap, CAP_PROBE_FILE])?;
        self.apply(&["rm", "-f", CAP_PROBE_FILE])?;
        if attempt.success() {
            return Err(failure(RustRunnerFailure::CapProbe));
        }
        self.apply(&["sh", "-c", RECORD_WRITER, "sh", &record, GUEST_CAP_RECORD])
    }

    fn runner_matches(&self, path: &str, digest: &str) -> Result<bool, HostError> {
        let owned = self
            .stat(&[path])?
            .get(path)
            .is_some_and(|entry| entry.is(REGULAR_FILE, 0o755));
        if !owned {
            return Ok(false);
        }
        let sum = self.read(&["sha256sum", path])?;
        Ok(sum.success() && parse::digest(&sum.text).as_deref() == Some(digest))
    }

    fn refresh_runner(&self, digest: &str) -> Result<(), HostError> {
        if self.runner_matches(GUEST_RUNNER, digest)? {
            return Ok(());
        }
        let shared = format!("{GUEST_SHARE}/bin/velnor-runner");
        self.apply(&[
            "install",
            "-o",
            "root",
            "-g",
            "root",
            "-m",
            "0755",
            &shared,
            RUNNER_STAGING,
        ])?;
        if !self.runner_matches(RUNNER_STAGING, digest)? {
            let _ = self.apply(&["rm", "-f", RUNNER_STAGING]);
            return Err(failure(RustRunnerFailure::BinaryCopy));
        }
        self.apply(&["mv", "-f", RUNNER_STAGING, GUEST_RUNNER])
    }
}
