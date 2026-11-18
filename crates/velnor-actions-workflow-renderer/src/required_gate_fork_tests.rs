// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A fork pull request must end with exactly one `Required` result that runs
//! (and fails), never with every `Required` job skipped, because a skipped
//! required check satisfies branch protection.

use super::*;

const SCALE: &str = "scale-set:velnor+ubuntu-26.04-scale-set";

struct Event {
    name: &'static str,
    same_repository: bool,
}

const EVENTS: [Event; 4] = [
    Event {
        name: "push",
        same_repository: true,
    },
    Event {
        name: "merge_group",
        same_repository: true,
    },
    Event {
        name: "pull_request",
        same_repository: true,
    },
    Event {
        name: "pull_request",
        same_repository: false,
    },
];

/// Evaluates the `if:` grammar the renderer emits: `&&`, `||`, `!`, parentheses,
/// `always()`, `cancelled()` and the two `github` comparisons.
fn evaluate(condition: &str, event: &Event) -> bool {
    let text = condition
        .trim()
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .unwrap_or(condition);
    let text = text
        .replace(
            "github.event.pull_request.head.repo.full_name == github.repository",
            &(event.name == "pull_request" && event.same_repository).to_string(),
        )
        .replace("always()", "true")
        .replace("cancelled()", "false");
    let mut text = text;
    for name in ["push", "merge_group", "pull_request"] {
        text = text.replace(
            &format!("github.event_name == '{name}'"),
            &(event.name == name).to_string(),
        );
    }
    let tokens: Vec<&str> = text
        .split_whitespace()
        .flat_map(|word| {
            let mut out = Vec::new();
            let mut rest = word;
            while !rest.is_empty() {
                let end = rest
                    .find(['(', ')', '!'])
                    .map_or(rest.len(), |i| if i == 0 { 1 } else { i });
                out.push(&rest[..end]);
                rest = &rest[end..];
            }
            out
        })
        .collect();
    let (value, used) = or_expr(&tokens, 0);
    assert_eq!(used, tokens.len(), "unparsed condition: {condition}");
    value
}

fn or_expr(tokens: &[&str], mut at: usize) -> (bool, usize) {
    let (mut value, next) = and_expr(tokens, at);
    at = next;
    while tokens.get(at) == Some(&"||") {
        let (rhs, next) = and_expr(tokens, at + 1);
        value |= rhs;
        at = next;
    }
    (value, at)
}

fn and_expr(tokens: &[&str], mut at: usize) -> (bool, usize) {
    let (mut value, next) = unary(tokens, at);
    at = next;
    while tokens.get(at) == Some(&"&&") {
        let (rhs, next) = unary(tokens, at + 1);
        value &= rhs;
        at = next;
    }
    (value, at)
}

fn unary(tokens: &[&str], at: usize) -> (bool, usize) {
    match tokens.get(at).copied() {
        Some("!") => {
            let (value, next) = unary(tokens, at + 1);
            (!value, next)
        }
        Some("(") => {
            let (value, next) = or_expr(tokens, at + 1);
            assert_eq!(tokens.get(next), Some(&")"), "unclosed group");
            (value, next + 1)
        }
        Some("true") => (true, at + 1),
        Some("false") => (false, at + 1),
        other => panic!("unexpected token {other:?}"),
    }
}

fn gate_jobs() -> Result<BTreeMap<String, Job>, RenderError> {
    let mut verdict = required_gate_job("ubuntu-26.04")?;
    verdict.display_name = REQUIRED_VERDICT_DISPLAY_NAME.to_owned();
    verdict.needs.clear();
    verdict.steps.clear();
    verdict.condition = Some(velnor_actions_contract::workflow::fork_fenced_condition(
        Some(REQUIRED_CONDITION),
    ));
    let mut gate = required_gate_job(SCALE)?;
    gate.condition = Some(fenced_gate_condition());
    let mut jobs = BTreeMap::from([
        (FINAL_JOB_ID.to_owned(), verdict),
        (REQUIRED_GATE_JOB_ID.to_owned(), gate),
    ]);
    add_fork_gate(&mut jobs, "ubuntu-26.04")?;
    Ok(jobs)
}

#[test]
fn every_event_runs_exactly_one_required_job() -> Result<(), RenderError> {
    let jobs = gate_jobs()?;
    check_required_gate(&jobs)?;
    let named: Vec<&Job> = jobs
        .values()
        .filter(|job| job.display_name == REQUIRED_DISPLAY_NAME)
        .collect();
    assert_eq!(named.len(), 2);
    for event in &EVENTS {
        let running = named
            .iter()
            .filter(|job| evaluate(job.condition.as_deref().unwrap_or("true"), event))
            .count();
        assert_eq!(
            running, 1,
            "{} (same repository: {}) must run exactly one Required job",
            event.name, event.same_repository
        );
    }
    Ok(())
}

#[test]
fn a_fork_pull_request_runs_only_the_hosted_twin_and_it_fails() -> Result<(), RenderError> {
    let jobs = gate_jobs()?;
    let fork = &EVENTS[3];
    let run = |id: &str| -> bool {
        jobs.get(id)
            .and_then(|job| job.condition.as_deref())
            .is_some_and(|condition| evaluate(condition, fork))
    };
    assert!(!run(REQUIRED_GATE_JOB_ID), "the scale-set gate must skip");
    assert!(run(REQUIRED_GATE_FORK_JOB_ID), "the hosted twin must run");
    assert!(
        !run(FINAL_JOB_ID),
        "the fenced verdict is skipped, so the twin sees a non-success result"
    );
    let twin = &jobs[REQUIRED_GATE_FORK_JOB_ID];
    assert!(!twin.runs_on.starts_with("scale-set:"));
    assert_eq!(
        twin.steps[0].condition.as_deref(),
        Some(REQUIRED_GATE_FAIL_CONDITION)
    );
    assert!(REQUIRED_GATE_FAIL_CONDITION.contains("!= 'success'"));
    Ok(())
}
