//! Crate-job disposition normalization cases.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, JobDisposition, MatrixEntry, ObligationDecision, PlanJob,
    PlanObligation, PlannedPlatform, digest_b3,
};

use super::{JOB_RUNS_REASON, NEEDED_REASON, finalize_dispositions};

fn obligation(task: &str, job: &str, decision: ObligationDecision) -> PlanObligation {
    PlanObligation {
        task_id: task.to_owned(),
        decision,
        reason: if decision == ObligationDecision::NotAffected {
            "not_affected"
        } else {
            "affected_by_change"
        }
        .to_owned(),
        task_digest: digest_b3(task.as_bytes()),
        input_digest: digest_b3(b"input"),
        closure_digest: digest_b3(b"closure"),
        job_id: job.to_owned(),
    }
}

fn entry(task: &str, job: &str) -> MatrixEntry {
    MatrixEntry::derive(
        "rust",
        task,
        "true",
        &digest_b3(task.as_bytes()),
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([("clippy".to_owned(), ExecuteTaskRef::Single(task.to_owned()))]),
        },
        &digest_b3(b"input"),
        "local",
        job,
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("platform"),
    )
    .expect("entry")
}

/// Plan rows: `(task, job, decision)`; every task gets one entry.
fn finalize(
    rows: &[(&str, &str, ObligationDecision)],
    needs: &[(&str, &str)],
) -> (Vec<PlanObligation>, Vec<MatrixEntry>, Vec<PlanJob>) {
    let mut obligations: Vec<PlanObligation> = rows
        .iter()
        .map(|(task, job, decision)| obligation(task, job, *decision))
        .collect();
    let mut entries: Vec<MatrixEntry> =
        rows.iter().map(|(task, job, _)| entry(task, job)).collect();
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (job, dep) in needs {
        graph
            .entry((*job).to_owned())
            .or_default()
            .insert((*dep).to_owned());
    }
    let jobs = finalize_dispositions(&mut obligations, &mut entries, &graph);
    (obligations, entries, jobs)
}

fn disposition(jobs: &[PlanJob], id: &str) -> JobDisposition {
    jobs.iter()
        .find(|job| job.job_id == id)
        .expect("job")
        .disposition
}

const NA: ObligationDecision = ObligationDecision::NotAffected;
const RUN: ObligationDecision = ObligationDecision::Execute;

#[test]
fn unaffected_crate_job_is_skipped_whole() {
    let (obligations, entries, jobs) = finalize(
        &[
            ("stack/rust/a/clippy/default", "rust-a", NA),
            ("stack/rust/a/doc/default", "rust-a", NA),
            ("stack/rust/b/clippy/default", "rust-b", RUN),
        ],
        &[],
    );
    assert_eq!(disposition(&jobs, "rust-a"), JobDisposition::NotAffected);
    assert_eq!(disposition(&jobs, "rust-b"), JobDisposition::Run);
    assert!(
        entries.iter().all(|entry| entry.job_id == "rust-b"),
        "skipped jobs keep no entries"
    );
    assert!(
        obligations
            .iter()
            .filter(|ob| ob.job_id == "rust-a")
            .all(|ob| ob.decision == NA)
    );
}

#[test]
fn mixed_job_runs_every_obligation() {
    let (obligations, entries, jobs) = finalize(
        &[
            ("stack/rust/a/clippy/default", "rust-a", NA),
            ("stack/rust/a/doc/default", "rust-a", RUN),
        ],
        &[],
    );
    assert_eq!(disposition(&jobs, "rust-a"), JobDisposition::Run);
    assert_eq!(entries.len(), 2);
    assert_eq!(obligations[0].decision, RUN);
    assert_eq!(obligations[0].reason, JOB_RUNS_REASON);
}

#[test]
fn jobs_a_running_job_needs_run_transitively() {
    let (obligations, _, jobs) = finalize(
        &[
            ("stack/rust/a/clippy/default", "rust-a", RUN),
            ("stack/rust/b/clippy/default", "rust-b", NA),
            ("stack/rust/c/clippy/default", "rust-c", NA),
            ("stack/rust/d/clippy/default", "rust-d", NA),
        ],
        &[
            ("rust-a", "rust-b"),
            ("rust-b", "rust-c"),
            ("rust-d", "rust-c"),
        ],
    );
    assert_eq!(disposition(&jobs, "rust-b"), JobDisposition::Run);
    assert_eq!(disposition(&jobs, "rust-c"), JobDisposition::Run);
    assert_eq!(
        disposition(&jobs, "rust-d"),
        JobDisposition::NotAffected,
        "a skipped job's needs stay skippable"
    );
    for job in ["rust-b", "rust-c"] {
        let reasons: Vec<&str> = obligations
            .iter()
            .filter(|ob| ob.job_id == job)
            .map(|ob| ob.reason.as_str())
            .collect();
        assert_eq!(reasons, [NEEDED_REASON], "{job}");
    }
}

#[test]
fn non_crate_jobs_always_run() {
    let (obligations, _, jobs) = finalize(
        &[
            ("stack/mise/x/check/default", "check-x", NA),
            ("stack/rust/root/fmt/default", "plan", NA),
            ("stack/tofu/infra/validate/default", "tofu-infra", NA),
        ],
        &[],
    );
    for job in ["check-x", "plan", "tofu-infra"] {
        assert_eq!(disposition(&jobs, job), JobDisposition::Run, "{job}");
    }
    assert!(
        obligations
            .iter()
            .all(|ob| ob.decision == RUN && ob.reason == JOB_RUNS_REASON)
    );
}
