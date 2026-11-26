// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::BackgroundTask;
use super::session::{Session, backoff};
use crate::exec::JobCancel;
use crate::protocol::{RunnerStatus, TaskAgentMessage};

const JOB_CANCELLATION: &str = "JobCancellation";

pub(super) type CancelPollHandle = BackgroundTask;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobCancelBody {
    #[serde(alias = "JobId")]
    job_id: String,
}

fn cancels_job(message: &TaskAgentMessage, job_id: &str) -> bool {
    message.message_type == JOB_CANCELLATION
        && serde_json::from_str::<JobCancelBody>(&message.body)
            .is_ok_and(|body| body.job_id.eq_ignore_ascii_case(job_id))
}

pub(super) fn spawn_cancel_poll(
    session: Arc<Session>,
    job_id: String,
    cancel: JobCancel,
    error_delay: Duration,
) -> CancelPollHandle {
    BackgroundTask::spawn(async move {
        let mut streak = 0_u32;
        loop {
            match session.poll(RunnerStatus::Busy).await {
                Ok(Some(message)) if cancels_job(&message, &job_id) => {
                    cancel.cancel();
                    return;
                }
                Ok(_) => streak = 0,
                Err(error) => {
                    streak += 1;
                    eprintln!("warning: poll for job cancellation failed: {error:#}");
                    tokio::time::sleep(backoff(error_delay, streak)).await;
                }
            }
        }
    })
}
