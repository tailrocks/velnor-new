// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::broker::{GIT_ENV, git_args};
use crate::exec::{ExecError, ProcessEnv, StepContext};

pub(super) async fn capture(
    ctx: &mut StepContext<'_>,
    git_dir: Option<&Path>,
    args: &[&str],
) -> Result<String, ExecError> {
    let args = git_args(git_dir, args);
    let env: Vec<(String, String)> = GIT_ENV
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    ctx.capture(("git", &args, ctx.workspace, ProcessEnv::Overlay(&env)))
        .await
}

pub(super) async fn read_blob(
    ctx: &mut StepContext<'_>,
    mirror: &Path,
    sha: &str,
    path: &str,
) -> Result<String, ExecError> {
    capture(ctx, Some(mirror), &["show", &format!("{sha}:{path}")]).await
}
