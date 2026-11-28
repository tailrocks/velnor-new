// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::broker::git_args;
use crate::exec::{ExecError, StepContext};

pub(super) async fn capture(
    ctx: &mut StepContext<'_>,
    git_dir: Option<&Path>,
    args: &[&str],
) -> Result<String, ExecError> {
    let bytes = capture_bytes(ctx, git_dir, args).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn capture_bytes(
    ctx: &mut StepContext<'_>,
    git_dir: Option<&Path>,
    args: &[&str],
) -> Result<Vec<u8>, ExecError> {
    let args = git_args(git_dir, args);
    ctx.capture_bytes(("git", &args, ctx.workspace, &[]), true)
        .await
}

pub(super) async fn read_blob(
    ctx: &mut StepContext<'_>,
    mirror: &Path,
    sha: &str,
    path: &str,
) -> Result<String, ExecError> {
    let bytes = capture_bytes(ctx, Some(mirror), &["show", &format!("{sha}:{path}")]).await?;
    String::from_utf8(bytes)
        .map_err(|_| ExecError::Invalid(format!("{path} in the trusted commit is not valid UTF-8")))
}
