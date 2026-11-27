// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, anyhow};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule,
};

const SYS_SETXATTRAT: i64 = 463;
const ABSENT: [i64; 7] = [
    libc::SYS_clone3,
    libc::SYS_io_uring_setup,
    libc::SYS_io_uring_enter,
    libc::SYS_io_uring_register,
    libc::SYS_add_key,
    libc::SYS_request_key,
    libc::SYS_keyctl,
];
const NO_XATTRS: [i64; 4] = [
    libc::SYS_setxattr,
    libc::SYS_lsetxattr,
    libc::SYS_fsetxattr,
    SYS_SETXATTRAT,
];

const BTRFS_FAMILY: u64 = 0x9400;
const FAMILY_MASK: u64 = 0xff00;
const REFLINK_REQUESTS: [u64; 3] = [0x4004_9409, 0x4020_940d, 0xc018_9436];

type Rules = Vec<(i64, Vec<SeccompRule>)>;

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

fn whole(calls: &[i64]) -> Rules {
    calls.iter().map(|call| (*call, Vec::new())).collect()
}

fn vsock() -> Result<Rules> {
    let family = u64::try_from(libc::AF_VSOCK).context("read AF_VSOCK")?;
    [libc::SYS_socket, libc::SYS_socketpair]
        .into_iter()
        .map(|call| {
            Ok((
                call,
                vec![argument(
                    0,
                    SeccompCmpArgLen::Dword,
                    SeccompCmpOp::Eq,
                    family,
                )?],
            ))
        })
        .collect()
}

fn btrfs_ioctls() -> Result<Rules> {
    let mut conditions = vec![
        SeccompCondition::new(
            1,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::MaskedEq(FAMILY_MASK),
            BTRFS_FAMILY,
        )
        .context("build the btrfs ioctl condition")?,
    ];
    for request in REFLINK_REQUESTS {
        conditions.push(
            SeccompCondition::new(1, SeccompCmpArgLen::Dword, SeccompCmpOp::Ne, request)
                .context("build a reflink exception")?,
        );
    }
    Ok(vec![(
        libc::SYS_ioctl,
        vec![SeccompRule::new(conditions).context("build the btrfs ioctl rule")?],
    )])
}

fn user_namespaces() -> Result<Rules> {
    let flag = u64::try_from(libc::CLONE_NEWUSER).context("read CLONE_NEWUSER")?;
    Ok(vec![
        (
            libc::SYS_clone,
            vec![argument(
                0,
                SeccompCmpArgLen::Qword,
                SeccompCmpOp::MaskedEq(flag),
                flag,
            )?],
        ),
        (
            libc::SYS_unshare,
            vec![argument(
                0,
                SeccompCmpArgLen::Dword,
                SeccompCmpOp::MaskedEq(flag),
                flag,
            )?],
        ),
    ])
}

pub(super) fn filters() -> Result<Vec<BpfProgram>> {
    Ok(vec![
        refuse(vsock()?, libc::EAFNOSUPPORT)?,
        refuse(user_namespaces()?, libc::EPERM)?,
        refuse(whole(&ABSENT), libc::ENOSYS)?,
        refuse(whole(&NO_XATTRS), libc::ENOTSUP)?,
        refuse(btrfs_ioctls()?, libc::ENOTTY)?,
    ])
}
