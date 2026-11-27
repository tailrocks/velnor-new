// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/tests.rs

use crate::expr_support::{empty_env, truthy};
use anyhow::Result;
use velnor_runner::expr::{ExprEnv, JobStatus, evaluate_condition};

#[test]
fn expr_status_functions_follow_job_status() -> Result<()> {
    let cases = [
        (JobStatus::Success, true, true, false),
        (JobStatus::Failure, false, true, false),
        (JobStatus::Cancelled, false, true, true),
    ];
    for (status, success, always, cancelled) in cases {
        let mut env: ExprEnv = empty_env();
        env.status = status;
        assert_eq!(truthy("success()", &env)?, success, "{status:?} success()");
        assert_eq!(truthy("always()", &env)?, always, "{status:?} always()");
        assert_eq!(
            truthy("CANCELLED()", &env)?,
            cancelled,
            "{status:?} cancelled()"
        );
        assert!(evaluate_condition("failure()", &env).is_err());
    }
    Ok(())
}

#[test]
fn expr_condition_empty_source_means_success() -> Result<()> {
    let mut env = empty_env();
    assert!(evaluate_condition("", &env)?);
    assert!(evaluate_condition("  ", &env)?);
    env.status = JobStatus::Failure;
    assert!(!evaluate_condition("", &env)?);
    assert!(evaluate_condition("always() && !success()", &env)?);
    assert!(!evaluate_condition("'' || null", &env)?);
    Ok(())
}
