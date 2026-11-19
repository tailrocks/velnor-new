// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::github::run_jobs_path;

use super::{GREEN, GREEN_CI, ONE_PASSED, Recorded, absent, concluded, verdict};
use crate::support::repo;

fn with_twin(conclusion: &str, twin_first: bool, own: &str) -> Recorded {
    Recorded::new().edit(&run_jobs_path(&repo(), GREEN_CI, 1), |page| {
        let jobs = page["jobs"]
            .as_array_mut()
            .unwrap_or_else(|| unreachable!());
        let at = jobs
            .iter()
            .position(|job| job["name"] == json!("Required"))
            .unwrap_or_else(|| unreachable!());
        jobs[at]["conclusion"] = json!(own);
        let mut twin: Value = jobs[at].clone();
        twin["conclusion"] = json!(conclusion);
        twin["id"] = json!(1);
        jobs.insert(if twin_first { at } else { at + 1 }, twin);
        page["total_count"] = json!(jobs.len());
    })
}

#[test]
fn a_skipped_required_job_beside_a_successful_one_is_ignored_in_either_order() {
    for twin_first in [true, false] {
        assert_eq!(
            with_twin("skipped", twin_first, "success").gate(GREEN),
            Ok(verdict(true, GREEN, ONE_PASSED))
        );
    }
}

#[test]
fn any_failed_required_job_fails_even_beside_a_skipped_one() {
    for twin_first in [true, false] {
        assert_eq!(
            with_twin("skipped", twin_first, "failure").gate(GREEN),
            concluded(GREEN, "Required", "failure")
        );
        assert_eq!(
            with_twin("failure", twin_first, "success").gate(GREEN),
            concluded(GREEN, "Required", "failure")
        );
    }
}

#[test]
fn only_skipped_required_jobs_never_pass() {
    assert_eq!(
        with_twin("skipped", true, "skipped").gate(GREEN),
        Err(absent("Required"))
    );
}
