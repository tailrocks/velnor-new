// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step, StepRole};

use crate::{RenderError, steps};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsrvSpec {
    pub package: String,
    pub rust_version: String,
}

impl MsrvSpec {
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.package.is_empty()
            || !self
                .package
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(RenderError::BadCommand(format!(
                "msrv_bad_package:{}",
                self.package
            )));
        }
        let mut parts = self.rust_version.split('.');
        let shaped = matches!((parts.next(), parts.next(), parts.next()), (Some(major), Some(minor), None)
            if !major.is_empty()
                && !minor.is_empty()
                && major.bytes().all(|b| b.is_ascii_digit())
                && minor.bytes().all(|b| b.is_ascii_digit()));
        if shaped {
            Ok(())
        } else {
            Err(RenderError::BadCommand(format!(
                "msrv_bad_rust_version:{}",
                self.rust_version
            )))
        }
    }
}

#[must_use]
pub fn msrv_step_name(package: &str) -> String {
    format!("msrv {package}")
}

pub fn msrv_step(
    spec: &MsrvSpec,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    spec.validate()?;
    let tool = format!("rust@{}", spec.rust_version);
    if !argv.iter().any(|arg| arg == &tool) {
        return Err(RenderError::BadCommand(
            "msrv_tool_not_rust_version".to_owned(),
        ));
    }
    if !argv.iter().any(|arg| arg == "--locked") {
        return Err(RenderError::BadCommand("msrv_without_locked".to_owned()));
    }
    let mut step = steps::shell_step(&msrv_step_name(&spec.package), argv, env)?;
    step.role = Some(StepRole::MsrvQualification);
    Ok(step)
}

pub fn msrv_job(
    label: &str,
    checkout_uses: &str,
    spec: &MsrvSpec,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Job, RenderError> {
    spec.validate()?;
    Ok(Job {
        services: Vec::new(),
        display_name: format!("MSRV {}", spec.package),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::MSRV,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: vec![
            steps::checkout_step(checkout_uses)?,
            msrv_step(spec, argv, env)?,
        ],
    })
}

pub fn check_no_msrv(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    for (id, job) in jobs {
        let leaking = id.to_lowercase().contains("msrv")
            || job
                .steps
                .iter()
                .any(|step| step.role == Some(StepRole::MsrvQualification));
        if leaking {
            return Err(RenderError::InvalidWorkflow(format!(
                "msrv_in_pr_workflow:{id}"
            )));
        }
    }
    Ok(())
}
