// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitCode};

use anyhow::{Context, Result, ensure};
use landlock::{CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetStatus, Scope};
use rustix::io::Errno;
use rustix::process::{Pid, Resource, Rlimit, WaitOptions, setrlimit, wait};
use rustix::thread::{CapabilitySet, capabilities, capability_is_in_bounding_set};
use rustix::thread::{remove_capability_from_bounding_set, set_capabilities};

use super::{JOB_WORK, Layout, seccomp};

const PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const FILE_SIZE_LIMIT: u64 = 16 << 30;
const KILLED_BASE: i32 = 128;

fn limit_file_size() -> Result<()> {
    setrlimit(
        Resource::Fsize,
        Rlimit {
            current: Some(FILE_SIZE_LIMIT),
            maximum: Some(FILE_SIZE_LIMIT),
        },
    )
    .context("limit the size of one file")
}

fn drop_capability_sets() -> Result<()> {
    for bit in 0..u64::BITS {
        let capability = CapabilitySet::from_bits_retain(1_u64 << bit);
        match capability_is_in_bounding_set(capability) {
            Ok(true) => remove_capability_from_bounding_set(capability)
                .context("drop a capability from the bounding set")?,
            Ok(false) => {}
            Err(Errno::INVAL) => break,
            Err(error) => return Err(error).context("read the bounding set"),
        }
    }
    let held = capabilities(None).context("read the capabilities")?;
    set_capabilities(
        None,
        rustix::thread::CapabilitySets {
            inheritable: CapabilitySet::empty(),
            ..held
        },
    )
    .context("clear the inheritable capabilities")
}

fn scope_abstract_sockets() -> Result<()> {
    let status = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .scope(Scope::AbstractUnixSocket)
        .context("request the abstract socket scope")?
        .create()
        .context("create the Landlock ruleset")?
        .restrict_self()
        .context("enforce the Landlock ruleset")?;
    ensure!(
        status.ruleset == RulesetStatus::FullyEnforced,
        "the abstract socket scope is not fully enforced"
    );
    Ok(())
}

fn restrict() -> Result<()> {
    limit_file_size()?;
    drop_capability_sets()?;
    rustix::thread::set_no_new_privs(true).context("set no_new_privs")?;
    scope_abstract_sockets()?;
    for filter in seccomp::filters()? {
        seccompiler::apply_filter(&filter).context("install a seccomp filter")?;
    }
    Ok(())
}

fn start(layout: &Layout, command: &[String]) -> Result<Child> {
    let (program, arguments) = command
        .split_first()
        .context("the sandbox has no command")?;
    Command::new(program)
        .args(arguments)
        .env_clear()
        .env("PATH", PATH)
        .env("HOME", &layout.home)
        .env("TMPDIR", "/tmp")
        .current_dir(JOB_WORK)
        .gid(layout.gid)
        .uid(layout.uid)
        .spawn()
        .with_context(|| format!("start {program}"))
}

fn reap(main: Pid) -> Result<i32> {
    loop {
        match wait(WaitOptions::empty()) {
            Ok(Some((pid, status))) if pid == main => {
                return Ok(status.exit_status().unwrap_or_else(|| {
                    KILLED_BASE.saturating_add(status.terminating_signal().unwrap_or(0))
                }));
            }
            Ok(_) | Err(Errno::INTR) => {}
            Err(error) => return Err(error).context("wait for the command"),
        }
    }
}

pub(super) fn supervise(layout: &Layout, command: &[String]) -> Result<ExitCode> {
    restrict()?;
    let child = start(layout, command)?;
    let null = File::open("/dev/null").context("open /dev/null")?;
    rustix::stdio::dup2_stdin(&null).context("release the standard input of the init")?;
    let main = i32::try_from(child.id())
        .ok()
        .and_then(Pid::from_raw)
        .context("read the pid of the command")?;
    let code = reap(main)?;
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(u8::MAX)))
}
