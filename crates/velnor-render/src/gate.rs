// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CI_AUTO_LABEL, CI_RUN_LABEL, Gate, GateNeed, JobId, MERGE_GROUP_SKIP_OUTPUT, MERGE_GROUP_STEP,
    PlanFrom, PrRun, RootSlug, VerdictSpec, When, group_output_name,
};

use crate::event_gate::event_gate;
use crate::rules::{RenderedStep, Value};

const SETUP_DONE: &str = "steps.restored.outcome == 'success'";
pub(crate) const PLAN_STEP: &str = "plan";
pub(crate) const CACHE_DEBUG_ON: &str = "vars.VELNOR_CACHE_DEBUG == '1'";
const NOT_CANCELLED: &str = "!cancelled()";
const ALWAYS: &str = "always()";
const NOT_DRAFT: &str = "github.event.pull_request.draft != true";
const IS_PULL_REQUEST_NOT: &str = "github.event_name != 'pull_request'";
const IS_PULL_REQUEST: &str = "github.event_name == 'pull_request'";
const PULL_REQUEST_LABELS: &str = "github.event.pull_request.labels.*.name";
const RELATED_EVENT: &str = "(github.event.action != 'labeled' || github.event.label.name == 'ci:run' || github.event.label.name == 'ci:auto') && (github.event.action != 'edited' || github.event.changes.base != null)";
const SAME_REPOSITORY: &str = "github.event.pull_request.head.repo.full_name == github.repository";
const POST_MERGE_EVENTS: &str = "(github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";
pub(crate) const NOT_SELF_HOSTED: &str = "runner.environment != 'self-hosted'";
pub(crate) const TOOLS_STEP: &str = "tools";
pub(crate) const TOOLS_LOCAL_STEP: &str = "toolslocal";
pub(crate) const TOOLS_LOCAL_SAVE_STEP: &str = "toolslocalsave";
pub(crate) const TOOLS_LOOKUP_STEP: &str = "toolslookup";
pub(crate) const TOOLS_RESTORED_STEP: &str = "tools_restored";
pub(crate) const TOOLS_SAVED_STEP: &str = "save_tools_node";
pub(crate) const PREPARE_COLD: &str = "steps.prepare.outputs.cold != 'true'";
pub(crate) const BASE_STEP: &str = "base";
pub(crate) const SOURCES_LOCKS_STEP: &str = "sources_locks";
pub(crate) const SAVING_EVENTS: &str = "(github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";
const NEEDS_JSON: &str = "NEEDS_JSON";
const GATE_SPEC: &str = "GATE_SPEC";
pub(crate) const ON_DEFAULT_BRANCH: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";
const ACTIVE_EVENT: &str = "(github.event_name != 'pull_request' || (startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed'))";
pub(crate) fn sources_step(lane: &RootSlug) -> String {
    format!("sources_{lane}")
}

pub(crate) fn sources_fetch_step(position: usize) -> String {
    format!("sources_fetch_{position}")
}

pub(crate) const SOURCES_FETCH_ALL_STEP: &str = "sources_fetch_all";

pub(crate) fn sources_save_step(lane: &RootSlug) -> String {
    format!("sources_save_{lane}")
}

pub(crate) fn verdict_step(spec: &VerdictSpec, needs_max_bytes: usize) -> RenderedStep {
    RenderedStep {
        name: "velnor-ci gate".to_owned(),
        env: vec![
            (
                NEEDS_JSON.to_owned(),
                Value::BoundedExpr {
                    text: "${{ toJSON(needs) }}".to_owned(),
                    max_bytes: needs_max_bytes,
                },
            ),
            (GATE_SPEC.to_owned(), Value::Text(spec.text())),
        ],
        run: Some(format!("velnor-ci gate --spec-env {GATE_SPEC}")),
        ..RenderedStep::default()
    }
}

pub(crate) fn output(slug: &RootSlug, need: GateNeed) -> String {
    need.output_name(slug)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Setup {
    Before,
    Restored,
    Replanned,
}

impl Setup {
    pub(crate) fn of(restores: bool, replanned: bool) -> Self {
        match (restores, replanned) {
            (false, _) => Self::Before,
            (true, false) => Self::Restored,
            (true, true) => Self::Replanned,
        }
    }
}

pub(crate) const REPLAN_STEP: &str = "replan";
const BASE_EXPIRED: &str = "steps.restored.outputs.expired == 'true'";

pub(crate) fn plan_read(source: &str, key: &str, setup: Setup) -> String {
    if setup == Setup::Replanned {
        format!("steps.{REPLAN_STEP}.outputs.{key} || {source}.{key}")
    } else {
        format!("{source}.{key}")
    }
}

pub(crate) fn plan_source(from: &PlanFrom) -> String {
    match from {
        PlanFrom::ThisJob => format!("steps.{PLAN_STEP}.outputs"),
        PlanFrom::Job(job) | PlanFrom::WorkJob(job) => format!("needs.{job}.outputs"),
    }
}

fn delta_conditions(when: When, setup: Setup, parts: &mut Vec<String>) {
    parts.push("github.event_name == 'pull_request'".to_owned());
    if when != When::PullRequest {
        parts.push(NOT_CANCELLED.to_owned());
        parts.push("github.event.pull_request.head.repo.full_name == github.repository".to_owned());
        parts.push(SETUP_DONE.to_owned());
        if setup == Setup::Replanned {
            parts.push(format!("steps.{REPLAN_STEP}.outcome != 'failure'"));
        }
    }
    if when == When::SaveDeltaDecided {
        parts.push("steps.delta.outputs.save == 'true'".to_owned());
    }
}

pub(crate) const MERGE_GROUP_ASKED_ON: &str = "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

pub(crate) fn merge_group_skipped(from: &PlanFrom, setup: Setup) -> String {
    let flag = match from {
        PlanFrom::ThisJob => format!("steps.{MERGE_GROUP_STEP}.outputs.skip != 'true'"),
        PlanFrom::Job(job) | PlanFrom::WorkJob(job) => {
            format!("needs.{job}.outputs.{MERGE_GROUP_SKIP_OUTPUT} != 'true'")
        }
    };
    if setup == Setup::Replanned {
        format!("({flag} || steps.{REPLAN_STEP}.outcome == 'success')")
    } else {
        flag
    }
}

pub(crate) fn condition(
    when: When,
    gate: Option<&Gate>,
    setup: Setup,
    pr: PrRun,
    extra: &[String],
    saved_after: &[String],
) -> Option<String> {
    let mut parts = Vec::new();
    match when {
        When::Success => {}
        When::Finalizer => parts.push(NOT_CANCELLED.to_owned()),
        When::Always => parts.push(ALWAYS.to_owned()),
        When::Failure => parts.push("failure()".to_owned()),
        When::NotCancelled | When::NotCancelledPostMerge => {
            parts.push(NOT_CANCELLED.to_owned());
            if setup != Setup::Before {
                parts.push(SETUP_DONE.to_owned());
            }
            parts.extend((when == When::NotCancelledPostMerge).then(|| POST_MERGE_EVENTS.to_owned()));
        }
        When::CacheDebug => {
            parts.push(NOT_CANCELLED.to_owned());
            parts.push(CACHE_DEBUG_ON.to_owned());
            if setup != Setup::Before {
                parts.push(SETUP_DONE.to_owned());
            }
        }
        When::Scheduled | When::ScheduledOrPush | When::DefaultBranchPush => {
            parts.push(event_gate(when));
        }
        When::ScheduledOrDefaultBranchPushAfterNeeds | When::DefaultBranchPushAfterNeeds => {
            parts.extend([NOT_CANCELLED.to_owned(), event_gate(when)]);
        }
        When::SaveBase | When::SaveBaseDecided | When::SaveSources => {
            parts.push(NOT_CANCELLED.to_owned());
            if setup != Setup::Before {
                parts.extend(crate::save_gate::settled(saved_after));
                parts.push(SETUP_DONE.to_owned());
            }
            if setup == Setup::Replanned {
                parts.push(format!("steps.{REPLAN_STEP}.outcome != 'failure'"));
            }
            parts.push(ON_DEFAULT_BRANCH.to_owned());
            parts.push(POST_MERGE_EVENTS.to_owned());
            if when == When::SaveSources {
                parts.push(format!("steps.{BASE_STEP}.outputs.sources == 'true'"));
            }
            if when == When::SaveBaseDecided {
                parts.push(format!("steps.{BASE_STEP}.outputs.save == 'true'"));
            }
        }
        When::FetchSources => {
            parts.push(NOT_CANCELLED.to_owned());
            if setup != Setup::Before {
                parts.push(SETUP_DONE.to_owned());
            }
            // The base decision only exists once the compile steps passed, so a fetch
            // that follows it needs no compile step of its own (one fetch per root).
            parts.extend(saved_after.is_empty().then(|| "success()".to_owned()));
            parts.push(format!("steps.{BASE_STEP}.outputs.sources == 'true'"));
        }
        When::FullAudit => {
            let expired = if setup == Setup::Before { "" } else { " || steps.restored.outputs.expired == 'true'" };
            parts.push(format!("(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'{expired})"));
        },
        When::FullAuditOrPush => parts.push("(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch' || github.event_name == 'push')".to_owned()),
        When::BaseExpired => parts.push(BASE_EXPIRED.to_owned()),
        When::SavingRun => {
            parts.push(ON_DEFAULT_BRANCH.to_owned());
            parts.push(POST_MERGE_EVENTS.to_owned());
        }
        When::ClearRunLabel => {
            parts.push(IS_PULL_REQUEST.to_owned());
            parts.push(SAME_REPOSITORY.to_owned());
            parts.push(has_label(CI_RUN_LABEL));
        }
        When::PullRequest | When::SaveDelta | When::SaveDeltaDecided => {
            delta_conditions(when, setup, &mut parts);
        }
        When::SaveToolsOnDefaultBranch
        | When::SaveToolsUnlessLocal
        | When::LookupToolsOnNodeHit
        | When::RestoreToolsUnlessLocal
        | When::PrimeToolsFromNetwork
        | When::SaveVerifiedTools(_) => crate::tools_steps::tools_parts(when, &mut parts),
    }
    parts.extend(pr_parts(pr));
    parts.extend(extra.iter().cloned());
    if let Some(gate) = gate {
        parts.extend(gate_parts(gate, setup == Setup::Replanned));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("${{{{ {} }}}}", parts.join(" && ")))
    }
}

fn has_label(label: &str) -> String {
    format!("contains({PULL_REQUEST_LABELS}, '{label}')")
}

fn requested_here() -> String {
    format!(
        "({SAME_REPOSITORY} && ({} || {}))",
        has_label(CI_RUN_LABEL),
        has_label(CI_AUTO_LABEL)
    )
}

pub(crate) fn check_name(name: Option<&String>, pr: PrRun) -> Option<String> {
    let name = name.filter(|_| pr == PrRun::Related)?;
    let quoted = name.replace('\'', "''");
    Some(format!(
        "${{{{ {RELATED_EVENT} && '{quoted}' || '{quoted} (ignored event)' }}}}"
    ))
}

fn pr_parts(pr: PrRun) -> Vec<String> {
    match pr {
        PrRun::Always => Vec::new(),
        PrRun::Ready => vec![NOT_DRAFT.to_owned()],
        PrRun::Quick => vec![NOT_DRAFT.to_owned(), RELATED_EVENT.to_owned()],
        PrRun::Related => vec![RELATED_EVENT.to_owned()],
        PrRun::Requested => vec![
            NOT_DRAFT.to_owned(),
            RELATED_EVENT.to_owned(),
            format!("({IS_PULL_REQUEST_NOT} || {})", requested_here()),
        ],
        PrRun::Unrequested => vec![
            IS_PULL_REQUEST.to_owned(),
            NOT_DRAFT.to_owned(),
            format!("!{}", requested_here()),
        ],
    }
}

pub(crate) fn admission(
    when: When,
    gate: Option<&Gate>,
    pr: PrRun,
    needs: &[JobId],
    skip: Option<&PlanFrom>,
) -> String {
    let mut plan_succeeded: Vec<String> = match gate.map(|gate| &gate.from) {
        Some(PlanFrom::Job(job)) => vec![format!("needs.{job}.result == 'success'")],
        Some(PlanFrom::WorkJob(_)) => vec![NOT_CANCELLED.to_owned()],
        _ => Vec::new(),
    };
    plan_succeeded.extend(crate::gate_needs::succeeded(when, needs));
    plan_succeeded.extend(skip.map(|from| merge_group_skipped(from, Setup::Before)));
    match condition(when, gate, Setup::Before, pr, &plan_succeeded, &[]) {
        Some(predicate) => format!(
            "${{{{ {ACTIVE_EVENT} && ({}) }}}}",
            predicate.trim_start_matches("${{ ").trim_end_matches(" }}")
        ),
        None => format!("${{{{ {ACTIVE_EVENT} }}}}"),
    }
}

pub(crate) fn all_conditions(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) => Some(format!(
            "${{{{ ({}) && ({}) }}}}",
            first.trim_start_matches("${{ ").trim_end_matches(" }}"),
            second.trim_start_matches("${{ ").trim_end_matches(" }}")
        )),
        (first, second) => first.or(second),
    }
}

pub(crate) fn gate_parts(gate: &Gate, replanned: bool) -> Vec<String> {
    let source = plan_source(&gate.from);
    let mut parts = match &gate.from {
        PlanFrom::ThisJob => vec![format!("steps.{PLAN_STEP}.outcome == 'success'")],
        PlanFrom::Job(_) | PlanFrom::WorkJob(_) => Vec::new(),
    };
    let read_key = |key: String| {
        if replanned {
            format!("({})", plan_read(&source, &key, Setup::Replanned))
        } else {
            plan_read(&source, &key, Setup::Restored)
        }
    };
    let read = |need| read_key(output(&gate.slug, need));
    let runs = |need| format!("{} == 'run'", read(need));
    match gate.need {
        GateNeed::Selected | GateNeed::Doctests | GateNeed::Docs => {
            parts.push(format!("{} != 'none'", read(GateNeed::Selected)));
            match gate.need {
                GateNeed::Doctests | GateNeed::Docs => {
                    parts.push(runs(gate.need));
                    parts.push(format!("{} != ''", read(GateNeed::PackageArgs)));
                }
                _ => {}
            }
        }
        GateNeed::Group => {
            parts.push(format!("{} != 'none'", read(GateNeed::Selected)));
            parts.extend(
                gate.group.iter().map(|name| {
                    format!("{} == 'run'", read_key(group_output_name(&gate.slug, name)))
                }),
            );
        }
        GateNeed::Deny => parts.push(runs(GateNeed::Deny)),
        GateNeed::Machete => parts.push(runs(GateNeed::Machete)),
        GateNeed::Policy => parts.push(format!(
            "({} || {})",
            runs(GateNeed::Deny),
            runs(GateNeed::Machete)
        )),
        GateNeed::PackageArgs if !gate.packages.is_empty() => {
            let args = format!("format(' {{0}} ', {})", read(GateNeed::PackageArgs));
            let selected = gate
                .packages
                .iter()
                .map(|package| format!("contains({args}, ' -p {package} ')"))
                .chain([format!("contains({args}, ' --workspace ')")])
                .collect::<Vec<_>>()
                .join(" || ");
            if gate.unselected {
                parts.push(format!("{} != 'none'", read(GateNeed::Selected)));
                parts.push(format!("!({selected})"));
            } else {
                parts.push(format!("({selected})"));
            }
        }
        GateNeed::TestFilter
        | GateNeed::PackageArgs
        | GateNeed::DoctestsKnown
        | GateNeed::TestsExpected => {
            parts.push(format!("{} != ''", read(gate.need)));
        }
    }
    parts
}
