// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::broker::{Request, Response, StoreWriter, Token, git_args};
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

pub(super) async fn ask(
    store: &StoreWriter,
    ctx: &mut StepContext<'_>,
    request: impl FnOnce(Option<Token>) -> Request,
) -> Result<(), ExecError> {
    let token = ctx.github_token.clone().map(Token::new);
    let pending = store.request(request(token));
    let response = match ctx.cancel.clone() {
        Some(cancel) => tokio::select! {
            response = pending => response,
            () = cancel.cancelled() => return Err(ExecError::Cancelled),
        },
        None => pending.await,
    };
    match response {
        Response::Done { notices } => {
            for notice in notices {
                ctx.log(&notice);
            }
            Ok(())
        }
        Response::Sound => Ok(()),
        Response::Failed { message } => Err(ExecError::Invalid(message)),
        Response::Hello(_)
        | Response::Ready
        | Response::Published { .. }
        | Response::AlreadyExists
        | Response::Denied(_) => Err(ExecError::Invalid(
            "store broker protocol error: unexpected answer to a mirror request".to_owned(),
        )),
    }
}
