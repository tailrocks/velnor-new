// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use super::Job;
use crate::exec::evaluate;
use crate::exec::output::Output;
use crate::exec::{AnnotationLevel, JobRun};
use crate::expr::JobStatus;
use crate::job_message::context_data::member;

impl Job<'_> {
    pub(super) async fn collect_environment_url(&mut self, out: &mut Output<'_>, run: &mut JobRun) {
        let Some(environment) = self
            .message
            .actions_environment
            .as_ref()
            .and_then(Value::as_object)
        else {
            return;
        };
        let Some(url) = member(environment, "url") else {
            return;
        };
        out.enter(0);
        let env = self.ctx.expr_env(self.status, &[]);
        match evaluate::scalar(url, &env).await {
            Ok(url) if out.masker.reveals(&url) => {
                let name = member(environment, "name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let warning = format!(
                    "Skip setting environment url as environment '{name}' may contain secret."
                );
                out.message(&warning);
                out.annotate(AnnotationLevel::Warning, warning);
            }
            Ok(url) => {
                out.message(&format!("Evaluated environment url: {url}"));
                run.environment_url = Some(url);
            }
            Err(error) => {
                out.message(&error.to_string());
                out.annotate(AnnotationLevel::Error, error.to_string());
                if self.status == JobStatus::Success {
                    self.status = JobStatus::Failure;
                }
            }
        }
    }
}
