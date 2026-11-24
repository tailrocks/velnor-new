// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::pairs::{Decision, consumed_agents, decisions, refused_among};
use velnor_local::scenario::{Metrics, Outcome};
use velnor_local::verdict::{Check, conclude, verdict};

const ADMITTED: &str = r#"{"pair":"trusted-1","side":"trusted","agent":"a1","allow":true,"reason":null,"ledger":"1-push","run_id":1,"attempt":1,"job":"ci","event":"push","ref":"refs/heads/main","sha":"abc"}"#;
const REFUSED: &str = r#"{"pair":"trusted-2","side":"trusted","agent":"a2","allow":false,"reason":"not_dispatched","ledger":null,"run_id":2,"attempt":1,"job":"ci","event":"push","ref":"refs/heads/main","sha":"abc"}"#;
const FORK: &str = r#"{"pair":"untrusted-1","side":"untrusted","agent":"a3","allow":true,"reason":null,"ledger":"3-pull_request","run_id":1,"attempt":1,"job":"lint","event":"pull_request","ref":"refs/pull/1/merge","sha":"def"}"#;
const MALFORMED: &str = r#"{"pair":"trusted-1","side":"trusted","agent":"a4","allow":false,"reason":"request_malformed","ledger":null,"run_id":null,"attempt":null,"job":null,"event":null,"ref":null,"sha":null}"#;

fn outcome(id: u64, trusted: bool) -> Outcome {
    Outcome {
        id,
        workflow: "mbx default".to_owned(),
        event: "push".to_owned(),
        conclusion: "success",
        started: Some(112),
        metrics: Metrics::Failed(String::new()),
        caches: Vec::new(),
        trusted,
        client_log: None,
    }
}

fn passing(id: u64, trusted: bool) -> Outcome {
    Outcome {
        metrics: Metrics::Written(std::path::PathBuf::new()),
        ..outcome(id, trusted)
    }
}

fn read(lines: &[&str]) -> Vec<Decision> {
    decisions(&lines.join("\n")).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn each_decision_reads_as_admitted_with_its_event_or_refused_with_its_reason() {
    let read = read(&[ADMITTED, REFUSED, "", MALFORMED]);
    let lines: Vec<String> = read.iter().map(Decision::line).collect();
    assert_eq!(
        lines,
        [
            "job ci: pair trusted-1, admitted (event 1-push)",
            "job ci: pair trusted-2, REFUSED at job start: not_dispatched",
            "job unknown: pair trusted-1, REFUSED at job start: request_malformed",
        ]
    );
    assert!(decisions("{not json").is_err());
}

#[test]
fn a_pair_verdict_puts_each_job_under_its_run_and_side_and_lists_the_rest() {
    let check = Check::Pairs(read(&[ADMITTED, FORK, MALFORMED]));
    let text = verdict(
        "warm",
        &[outcome(1, true)],
        (100, 9.8),
        &check,
        velnor_local::authority::WARNING,
    );
    let lines: Vec<&str> = text.lines().collect();
    let run = lines
        .iter()
        .position(|line| line.contains("run 1 mbx default (push): success"))
        .unwrap_or_else(|| unreachable!("{text}"));
    assert_eq!(
        lines[run + 1],
        "velnor-local warm:     job ci: pair trusted-1, admitted (event 1-push)"
    );
    assert!(text.contains("velnor-local warm: Job-start decisions outside these runs:\nvelnor-local warm:   untrusted side, run 1: job lint: pair untrusted-1, admitted (event 3-pull_request)\nvelnor-local warm:   trusted side, no run: job unknown: pair trusted-1, REFUSED at job start: request_malformed\n"), "{text}");
    assert!(
        text.contains("first job started 12 s after up began"),
        "{text}"
    );
    for part in [
        "executed: official runner 2.338.0 in velnor-host pairs (private Docker, limits, job-start check)",
        "Stood in: jobs and job-start facts by runner.server and the driver's ledger",
        "Not exercised: scale set acquire, GitHub JIT, journal, GitHub runner deletion proof",
    ] {
        assert!(text.contains(part), "{part}\n{text}");
    }
    assert!(!text.contains("plain containers"), "{text}");
    assert!(check.refused());
    assert!(!Check::Pairs(read(&[ADMITTED, FORK])).refused());
}

#[test]
fn a_plain_verdict_says_there_is_no_job_start_check() {
    let text = verdict(
        "warm",
        &[outcome(1, true)],
        (100, 9.8),
        &Check::Plain,
        velnor_local::authority::WARNING,
    );
    assert!(
        text.contains("job-start check: none (plain containers)"),
        "{text}"
    );
    assert!(!text.contains("pair trusted"), "{text}");
    assert!(!Check::Plain.refused());
}

#[test]
fn a_refused_job_fails_the_run_even_when_every_run_succeeded() {
    let passed = [passing(1, true)];
    let refused = Check::Pairs(read(&[ADMITTED, REFUSED]));
    assert_eq!(
        conclude(&refused, &passed),
        Err("a job was refused at job start")
    );
    assert_eq!(conclude(&Check::Pairs(read(&[ADMITTED])), &passed), Ok(()));
    assert_eq!(conclude(&Check::Plain, &passed), Ok(()));
}

#[test]
fn a_pair_run_without_an_admitted_decision_for_a_job_fails_the_run() {
    let outcomes = [passing(1, true), passing(1, false)];
    let missing = "a job has no job-start decision";
    assert_eq!(conclude(&Check::Pairs(Vec::new()), &outcomes), Err(missing));
    assert_eq!(
        conclude(&Check::Pairs(read(&[ADMITTED])), &outcomes),
        Err(missing)
    );
    assert_eq!(
        conclude(&Check::Pairs(read(&[ADMITTED, FORK])), &outcomes),
        Ok(())
    );
}

#[test]
fn a_run_that_did_not_succeed_fails_the_run() {
    let mut failed = outcome(1, false);
    failed.conclusion = "failure";
    assert_eq!(
        conclude(&Check::Plain, &[failed]),
        Err("a run did not succeed")
    );
}

#[test]
fn a_refusal_counts_only_for_the_runs_the_event_created_on_its_side() {
    let line = |side: &str, run: u64, allow: bool| Decision {
        pair: format!("{side}-1"),
        side: side.to_owned(),
        allow,
        reason: (!allow).then(|| "policy".to_owned()),
        ledger: None,
        run_id: Some(run),
        job: Some("ci".to_owned()),
    };
    let all = [line("trusted", 4, false), line("untrusted", 9, false)];
    assert!(refused_among(&all, true, &[4, 5]));
    assert!(!refused_among(&all, true, &[5, 6]));
    assert!(!refused_among(&all, true, &[9]));
    assert!(refused_among(&all, false, &[9]));
    assert!(!refused_among(&[line("trusted", 5, true)], true, &[5]));
}

#[test]
fn every_agent_that_took_a_job_start_decision_is_consumed_even_when_refused() {
    let text = [ADMITTED, REFUSED, FORK, ""].join("\n");
    let consumed = consumed_agents(&text).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(consumed.into_iter().collect::<Vec<_>>(), ["a1", "a2", "a3"]);
    assert!(consumed_agents("").is_ok_and(|none| none.is_empty()));
    assert!(consumed_agents("{not json").is_err());
}

#[test]
fn the_plain_verdict_names_the_ubuntu_release_of_the_runner_dockerfile() {
    let dockerfile = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/docker/runner/Dockerfile"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let release = dockerfile
        .lines()
        .find_map(|line| line.strip_prefix("FROM ubuntu:"))
        .and_then(|base| base.split('@').next())
        .unwrap_or_else(|| unreachable!("no ubuntu base in the runner Dockerfile"));
    let text = verdict(
        "warm",
        &[outcome(1, true)],
        (100, 9.8),
        &Check::Plain,
        velnor_local::authority::WARNING,
    );
    assert!(
        text.contains(&format!(
            "every Ubuntu label ran on the one Ubuntu {release} runner image"
        )),
        "{text}"
    );
}
