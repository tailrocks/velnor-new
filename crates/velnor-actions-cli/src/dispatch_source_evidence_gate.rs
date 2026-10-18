//! Runner-context gate for qualification source evidence.

use std::env;

use super::{GITHUB_EVENT_ENV, InternalOp, InternalRequest, run_context, runner_velnor_dir};

pub(super) fn request(op: InternalOp) -> Option<InternalRequest> {
    if !run_context::valid_run_coordinates()
        || !env::var(GITHUB_EVENT_ENV).is_ok_and(|event| event == "workflow_dispatch")
        || !matches!(
            env::var("GITHUB_REF_PROTECTED").as_deref(),
            Ok("true" | "false")
        )
        || [
            "GITHUB_EVENT_PATH",
            "GITHUB_WORKSPACE",
            "GITHUB_JOB",
            "GITHUB_REPOSITORY",
            "GITHUB_REF",
            "GITHUB_WORKFLOW_REF",
            "GITHUB_WORKFLOW_SHA",
            "GITHUB_SHA",
        ]
        .into_iter()
        .any(|name| env::var_os(name).is_none_or(|value| value.is_empty()))
    {
        return None;
    }
    runner_velnor_dir().map(|path| InternalRequest { op, path })
}
