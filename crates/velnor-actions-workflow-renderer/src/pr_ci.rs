// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Opt-in label-triggered pull-request CI (`[execution] pr_ci = "label"`).
//!
//! The transform runs on the finished workflow document, after every IR
//! validator has accepted the default shape, so the default output stays
//! byte-identical and the label mode only rewrites triggers, concurrency and
//! job conditions.

use velnor_actions_contract::{PrCiMode, ValidatorKind};

use crate::RenderError;
use crate::yaml::Yaml;

pub const RUN_LABEL: &str = "ci:run";
pub const AUTO_LABEL: &str = "ci:auto";
pub const CLAIM_JOB_ID: &str = "pr-ci-claim";
pub const PR_TYPES: &[&str] = &[
    "opened",
    "synchronize",
    "reopened",
    "ready_for_review",
    "labeled",
];

const REQUIRED_JOB: &str = "required";
const REQUIRED_GATE_JOB: &str = "required-gate";
const PLAN_JOB: &str = "plan";

/// Draft pull requests and unrelated label events run nothing.
pub const ADMIT: &str = "github.event_name != 'pull_request' || (github.event.pull_request.draft == false && (github.event.action != 'labeled' || github.event.label.name == 'ci:run' || github.event.label.name == 'ci:auto'))";

/// Everything except a same-repository pull request that carries a request
/// label runs the full suite.
pub const FULL: &str = "github.event_name != 'pull_request' || (github.event.pull_request.head.repo.fork == false && (contains(github.event.pull_request.labels.*.name, 'ci:run') || contains(github.event.pull_request.labels.*.name, 'ci:auto')))";

const CLAIM_CONDITION: &str = "github.event_name == 'pull_request' && github.event.pull_request.head.repo.fork == false && github.event.pull_request.draft == false && contains(github.event.pull_request.labels.*.name, 'ci:run')";

const IGNORED_SUFFIX: &str = "${{ github.event.action == 'labeled' && github.event.label.name != 'ci:run' && github.event.label.name != 'ci:auto' && format('-ignored-{0}', github.run_id) || '' }}";

const CLAIM_SCRIPT: &str = "code=$(curl -sS -o /dev/null -w '%{http_code}' -X DELETE -H \"Authorization: Bearer $GH_TOKEN\" -H 'Accept: application/vnd.github+json' -H 'X-GitHub-Api-Version: 2022-11-28' \"$GITHUB_API_URL/repos/$GITHUB_REPOSITORY/issues/$PR_NUMBER/labels/ci%3Arun\"); case \"$code\" in 200|404) : ;; *) echo \"ci:run removal failed: HTTP $code\" >&2; exit 1 ;; esac";

#[derive(Debug, Clone, Copy)]
pub struct PrCiShape<'a> {
    pub quick: &'a [&'a str],
    pub claim: bool,
    /// Also trigger on `merge_group`; only for workflows that can run there.
    pub merge_group: bool,
}

#[must_use]
pub fn validator_job_ids() -> Vec<&'static str> {
    ValidatorKind::all()
        .iter()
        .map(ValidatorKind::job_id)
        .collect()
}

pub fn apply(document: &mut Yaml, mode: PrCiMode, shape: PrCiShape<'_>) -> Result<(), RenderError> {
    if mode.is_off() {
        return Ok(());
    }
    let Yaml::Map(root) = document else {
        return Err(bad("document"));
    };
    if !set_pr_types(root, shape.merge_group) {
        return Ok(());
    }
    set_concurrency(root)?;
    let Some((_, Yaml::Map(jobs))) = root.iter_mut().find(|(key, _)| key == "jobs") else {
        return Err(bad("jobs"));
    };
    let present: Vec<String> = jobs.iter().map(|(id, _)| id.clone()).collect();
    let quick: Vec<&str> = shape
        .quick
        .iter()
        .copied()
        .filter(|id| present.iter().any(|have| have == id))
        .collect();
    let claim_runs_on = jobs
        .iter()
        .find(|(id, _)| id == PLAN_JOB)
        .and_then(|(_, job)| field(job, "runs-on").cloned());
    for (id, job) in jobs.iter_mut() {
        let Yaml::Map(fields) = job else {
            return Err(bad("job"));
        };
        if id == REQUIRED_GATE_JOB {
            gate_job(fields, &quick)?;
        } else if quick.contains(&id.as_str()) {
            wrap_if(fields, &format!("({ADMIT})"));
        } else {
            wrap_if(fields, &format!("({ADMIT}) && ({FULL})"));
        }
    }
    if shape.claim {
        let runs_on = claim_runs_on.ok_or_else(|| bad("claim_runs_on"))?;
        jobs.push((CLAIM_JOB_ID.to_owned(), claim_job(runs_on)));
    }
    Ok(())
}

fn bad(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("pr_ci:{problem}"))
}

fn field<'a>(node: &'a Yaml, name: &str) -> Option<&'a Yaml> {
    match node {
        Yaml::Map(fields) => fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value),
        _ => None,
    }
}

fn set_pr_types(root: &mut [(String, Yaml)], merge_group: bool) -> bool {
    let Some((_, Yaml::Map(on))) = root.iter_mut().find(|(key, _)| key == "on") else {
        return false;
    };
    if !on.iter().any(|(key, _)| key == "pull_request") {
        return false;
    }
    if merge_group && !on.iter().any(|(key, _)| key == "merge_group") {
        on.push(("merge_group".to_owned(), Yaml::Null));
    }
    let Some((_, Yaml::Map(pull_request))) = on.iter_mut().find(|(key, _)| key == "pull_request")
    else {
        return false;
    };
    let types = Yaml::Seq(PR_TYPES.iter().map(|kind| Yaml::str(*kind)).collect());
    if let Some((_, slot)) = pull_request.iter_mut().find(|(key, _)| key == "types") {
        *slot = types;
    } else {
        pull_request.push(("types".to_owned(), types));
    }
    true
}

fn set_concurrency(root: &mut [(String, Yaml)]) -> Result<(), RenderError> {
    let Some((_, Yaml::Map(concurrency))) = root.iter_mut().find(|(key, _)| key == "concurrency")
    else {
        return Err(bad("concurrency"));
    };
    for (key, value) in concurrency.iter_mut() {
        match (key.as_str(), &*value) {
            ("group", Yaml::Str(group)) => {
                let base = if group.contains("github.event.pull_request.number") {
                    group.clone()
                } else {
                    group.replace(
                        "${{ github.ref }}",
                        "${{ github.event.pull_request.number || github.ref }}",
                    )
                };
                *value = Yaml::Str(format!("{base}{IGNORED_SUFFIX}"));
            }
            ("cancel-in-progress", Yaml::Bool(false)) => {
                *value = Yaml::str("${{ github.event_name == 'pull_request' }}");
            }
            _ => {}
        }
    }
    Ok(())
}

fn strip_expression(text: &str) -> &str {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .map_or(trimmed, str::trim)
}

fn wrap_if(fields: &mut Vec<(String, Yaml)>, extra: &str) {
    let existing = fields
        .iter()
        .position(|(key, _)| key == "if")
        .and_then(|at| match &fields[at].1 {
            Yaml::Str(text) => Some((at, text.clone())),
            _ => None,
        });
    if let Some((at, old)) = existing {
        let combined = format!("({}) && {extra}", strip_expression(&old));
        fields[at].1 = Yaml::Str(format!("${{{{ {combined} }}}}"));
    } else {
        let at = fields
            .iter()
            .position(|(key, _)| key == "steps")
            .unwrap_or(fields.len());
        fields.insert(
            at,
            ("if".to_owned(), Yaml::Str(format!("${{{{ {extra} }}}}"))),
        );
    }
}

fn gate_job(fields: &mut [(String, Yaml)], quick: &[&str]) -> Result<(), RenderError> {
    let mut needs = vec![REQUIRED_JOB.to_owned()];
    needs.extend(quick.iter().map(|id| (*id).to_owned()));
    let quick_failed = quick
        .iter()
        .map(|id| format!("needs.{id}.result == 'failure' || needs.{id}.result == 'cancelled'"))
        .collect::<Vec<_>>()
        .join(" || ");
    let verdict = if quick_failed.is_empty() {
        "needs.required.result != 'success'".to_owned()
    } else {
        format!(
            "(({FULL}) && needs.required.result != 'success') || (!({FULL}) && ({quick_failed}))"
        )
    };
    for (key, value) in fields.iter_mut() {
        match key.as_str() {
            "needs" => *value = Yaml::Seq(needs.iter().cloned().map(Yaml::str).collect()),
            "if" => *value = Yaml::str(format!("${{{{ always() && ({ADMIT}) }}}}")),
            "steps" => {
                let Yaml::Seq(steps) = value else {
                    return Err(bad("gate_steps"));
                };
                for step in steps {
                    let Yaml::Map(step_fields) = step else {
                        return Err(bad("gate_step"));
                    };
                    for (step_key, step_value) in step_fields.iter_mut() {
                        if step_key == "if" {
                            *step_value = Yaml::str(format!("${{{{ always() && ({verdict}) }}}}"));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn claim_job(runs_on: Yaml) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Claim one-shot label")),
        ("runs-on".to_owned(), runs_on),
        ("timeout-minutes".to_owned(), Yaml::Int(5)),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("pull-requests".to_owned(), Yaml::str("write"))]),
        ),
        (
            "if".to_owned(),
            Yaml::str(format!("${{{{ {CLAIM_CONDITION} }}}}")),
        ),
        (
            "steps".to_owned(),
            Yaml::Seq(vec![Yaml::Map(vec![
                ("name".to_owned(), Yaml::str("Remove the ci:run label")),
                (
                    "env".to_owned(),
                    Yaml::Map(vec![
                        ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
                        (
                            "PR_NUMBER".to_owned(),
                            Yaml::str("${{ github.event.pull_request.number }}"),
                        ),
                    ]),
                ),
                ("run".to_owned(), Yaml::str(CLAIM_SCRIPT)),
            ])]),
        ),
    ])
}
