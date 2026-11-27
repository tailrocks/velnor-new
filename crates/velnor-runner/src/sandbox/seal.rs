// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::convert::Infallible;
use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{Context, Result, anyhow, ensure};
use landlock::{CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetStatus, Scope};
use rustix::process::{Resource, Rlimit, setrlimit};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule,
};

use super::Layout;

const PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const FILE_SIZE_LIMIT: u64 = 16 << 30;

type Rules = Vec<(i64, Vec<SeccompRule>)>;

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

fn argument(
    index: u8,
    width: SeccompCmpArgLen,
    operation: SeccompCmpOp,
    value: u64,
) -> Result<SeccompRule> {
    let condition = SeccompCondition::new(index, width, operation, value)
        .context("build a seccomp condition")?;
    SeccompRule::new(vec![condition]).context("build a seccomp rule")
}

fn refuse(rules: Rules, errno: i32) -> Result<BpfProgram> {
    SeccompFilter::new(
        rules.into_iter().collect(),
        SeccompAction::Allow,
        SeccompAction::Errno(u32::try_from(errno).context("read the errno")?),
        std::env::consts::ARCH
            .try_into()
            .map_err(|error| anyhow!("seccomp does not support this architecture: {error}"))?,
    )
    .context("build a seccomp filter")?
    .try_into()
    .context("compile a seccomp filter")
}

pub(super) fn filters() -> Result<Vec<BpfProgram>> {
    let vsock = u64::try_from(libc::AF_VSOCK).context("read AF_VSOCK")?;
    let new_user = u64::try_from(libc::CLONE_NEWUSER).context("read CLONE_NEWUSER")?;
    let mut sockets = Rules::new();
    for call in [libc::SYS_socket, libc::SYS_socketpair] {
        sockets.push((
            call,
            vec![argument(
                0,
                SeccompCmpArgLen::Dword,
                SeccompCmpOp::Eq,
                vsock,
            )?],
        ));
    }
    let namespaces: Rules = vec![
        (
            libc::SYS_clone,
            vec![argument(
                0,
                SeccompCmpArgLen::Qword,
                SeccompCmpOp::MaskedEq(new_user),
                new_user,
            )?],
        ),
        (
            libc::SYS_unshare,
            vec![argument(
                0,
                SeccompCmpArgLen::Dword,
                SeccompCmpOp::MaskedEq(new_user),
                new_user,
            )?],
        ),
    ];
    Ok(vec![
        refuse(sockets, libc::EAFNOSUPPORT)?,
        refuse(namespaces, libc::EPERM)?,
        refuse(vec![(libc::SYS_clone3, Vec::new())], libc::ENOSYS)?,
    ])
}

pub(super) fn exec(layout: &Layout, command: &[String]) -> Result<Infallible> {
    limit_file_size()?;
    rustix::thread::set_no_new_privs(true).context("set no_new_privs")?;
    scope_abstract_sockets()?;
    for filter in filters()? {
        seccompiler::apply_filter(&filter).context("install a seccomp filter")?;
    }
    let (program, arguments) = command
        .split_first()
        .context("the sandbox has no command")?;
    let error = Command::new(program)
        .args(arguments)
        .env_clear()
        .env("PATH", PATH)
        .env("HOME", &layout.home)
        .env("TMPDIR", "/tmp")
        .current_dir(&layout.work_dir)
        .gid(layout.gid)
        .uid(layout.uid)
        .exec();
    Err(error).with_context(|| format!("start {program}"))
}
