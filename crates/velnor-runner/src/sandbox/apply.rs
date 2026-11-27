// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;

use anyhow::Result;

use super::ApplyArgs;

#[cfg(target_os = "linux")]
pub fn apply(args: &ApplyArgs) -> Result<ExitCode> {
    use anyhow::Context;

    use super::{Layout, Profile, cgroup, mounts, plan, seal};

    let layout: Layout = serde_json::from_str(&args.layout).context("parse the sandbox layout")?;
    cgroup::enter(&layout.cgroup)?;
    mounts::build(&layout, &plan(Profile::Job, &layout))?;
    seal::supervise(&layout, &args.command)
}

#[cfg(not(target_os = "linux"))]
pub fn apply(_args: &ApplyArgs) -> Result<ExitCode> {
    anyhow::bail!("the sandbox needs Linux")
}
