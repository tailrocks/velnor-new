// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::preview::json::Res;
use crate::preview::sealed::Plan;

fn rejected() -> CiError {
    CiError::new("source preview executable rejected")
}

/// Port of `source_preview_run_verified.py`: copy the verified Linux SDK into a sealed
/// memory file, check the copy's digest, and replace this process with it. Only returns
/// on failure.
#[cfg(target_os = "linux")]
pub fn exec(plan: &Plan) -> Res<std::convert::Infallible> {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::os::unix::process::CommandExt;

    use nix::fcntl::{FcntlArg, FdFlag, SealFlag, fcntl};
    use nix::sys::memfd::{MFdFlags, memfd_create};
    use nix::unistd::geteuid;

    use crate::preview::json::{digest, require};
    use crate::preview::sealed::MAX_EXECUTABLE;

    let mut source = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&plan.source)
        .map_err(|_| rejected())?;
    let meta = source.metadata().map_err(|_| rejected())?;
    require(
        meta.is_file()
            && meta.uid() == geteuid().as_raw()
            && meta.len() > 0
            && meta.len() <= MAX_EXECUTABLE,
    )?;
    let sealed = memfd_create("velnor-approved-preview", MFdFlags::MFD_ALLOW_SEALING)
        .map_err(|_| rejected())?;
    let mut copy = std::fs::File::from(sealed);
    let mut data = Vec::with_capacity(usize::try_from(meta.len()).map_err(|_| rejected())?);
    (&mut source)
        .take(meta.len() + 1)
        .read_to_end(&mut data)
        .map_err(|_| rejected())?;
    require(data.len() as u64 == meta.len() && digest(&data) == plan.expected_sha256)?;
    copy.write_all(&data).map_err(|_| rejected())?;
    let seals = SealFlag::F_SEAL_WRITE
        | SealFlag::F_SEAL_GROW
        | SealFlag::F_SEAL_SHRINK
        | SealFlag::F_SEAL_SEAL;
    fcntl(&copy, FcntlArg::F_ADD_SEALS(seals)).map_err(|_| rejected())?;
    require(fcntl(&copy, FcntlArg::F_GET_SEALS).map_err(|_| rejected())? == seals.bits())?;
    fcntl(&copy, FcntlArg::F_SETFD(FdFlag::empty())).map_err(|_| rejected())?;
    let executable = format!("/proc/self/fd/{}", copy.as_raw_fd());
    let error = std::process::Command::new(&executable)
        .arg0(&executable)
        .args(&plan.args)
        .env_clear()
        .envs(&plan.env)
        .exec();
    let _ = error;
    Err(rejected())
}

#[cfg(not(target_os = "linux"))]
pub fn exec(_plan: &Plan) -> Res<std::convert::Infallible> {
    Err(rejected())
}
