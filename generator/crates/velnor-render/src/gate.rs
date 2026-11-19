// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CI_AUTO_LABEL, CI_RUN_LABEL, Gate, GateNeed, MERGE_GROUP_SKIP_OUTPUT, MERGE_GROUP_STEP,
    PlanFrom, PrRun, RootSlug, VerdictSpec, When, group_output_name,
};

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
const SCHEDULE: &str = "schedule";
const PUSH: &str = "push";
const POST_MERGE_EVENTS: &str = "(github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";
pub(crate) const TOOLS_STEP: &str = "tools";
pub(crate) const TOOLS_LOCAL_STEP: &str = "toolslocal";
pub(crate) const BASE_STEP: &str = "base";
pub(crate) const SOURCES_LOCKS_STEP: &str = "sources_locks";
const SAVING_EVENTS: &str = "(github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";
const NEEDS_JSON: &str = "NEEDS_JSON";
const GATE_SPEC: &str = "GATE_SPEC";
const ON_DEFAULT_BRANCH: &str =
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
        PlanFrom::Job(job) => format!("needs.{job}.outputs"),
    }
}

pub(crate) const MERGE_GROUP_ASKED_ON: &str = "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

pub(crate) fn merge_group_skipped(from: &PlanFrom) -> String {
    match from {
        PlanFrom::ThisJob => format!("steps.{MERGE_GROUP_STEP}.outputs.skip != 'true'"),
        PlanFrom::Job(job) => {
            format!("needs.{job}.outputs.{MERGE_GROUP_SKIP_OUTPUT} != 'true'")
        }
    }
}

pub(crate) fn condition(
    when: When,
    gate: Option<&Gate>,
    setup: Setup,
    pr: PrRun,
    extra: &[String],
) -> Option<String> {
    let mut parts = Vec::new();
    match when {
        When::Success => {}
        When::Finalizer => parts.push(NOT_CANCELLED.to_owned()),
        When::Always => parts.push(ALWAYS.to_owned()),
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
        When::Scheduled => parts.push(format!("github.event_name == '{SCHEDULE}'")),
        When::ScheduledOrPush => parts.push(format!(
            "(github.event_name == '{SCHEDULE}' || github.event_name == '{PUSH}')"
        )),
        When::SaveBase | When::SaveBaseDecided | When::SaveSources => {
            parts.push(NOT_CANCELLED.to_owned());
            if setup != Setup::Before {
                parts.push("success()".to_owned());
                parts.push(SETUP_DONE.to_owned());
            }
            if setup == Setup::Replanned {
                parts.push(format!("steps.{REPLAN_STEP}.outcome != 'failure'"));
            }
            parts.push(ON_DEFAULT_BRANCH.to_owned());
            parts.push(SAVING_EVENTS.to_owned());
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
            parts.push("success()".to_owned());
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
            parts.push(SAVING_EVENTS.to_owned());
        }
        When::ClearRunLabel => {
            parts.push(IS_PULL_REQUEST.to_owned());
            parts.push(SAME_REPOSITORY.to_owned());
            parts.push(has_label(CI_RUN_LABEL));
        }
        When::SaveToolsOnDefaultBranch | When::SaveToolsUnlessLocal => {
            parts.push(ON_DEFAULT_BRANCH.to_owned());
            parts.push(format!("steps.{TOOLS_STEP}.outputs.cache-hit != 'true'"));
            if when == When::SaveToolsUnlessLocal {
                parts.push(format!("steps.{TOOLS_LOCAL_STEP}.outputs.hit != 'true'"));
            }
        }
        When::RestoreToolsUnlessLocal => {
            parts.push(format!("steps.{TOOLS_LOCAL_STEP}.outputs.hit != 'true'"));
        }
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

pub(crate) fn admission(when: When, gate: Option<&Gate>, pr: PrRun) -> String {
    let plan_succeeded: Vec<String> = match gate.map(|gate| &gate.from) {
        Some(PlanFrom::Job(job)) => vec![format!("needs.{job}.result == 'success'")],
        _ => Vec::new(),
    };
    match condition(when, gate, Setup::Before, pr, &plan_succeeded) {
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

fn gate_parts(gate: &Gate, replanned: bool) -> Vec<String> {
    let source = plan_source(&gate.from);
    let mut parts = match &gate.from {
        PlanFrom::ThisJob => vec![format!("steps.{PLAN_STEP}.outcome == 'success'")],
        PlanFrom::Job(_) => Vec::new(),
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
        GateNeed::PackageArgs if gate.package.is_some() => {
            parts.extend(gate.package.iter().map(|package| {
                let args = format!("format(' {{0}} ', {})", read(GateNeed::PackageArgs));
                format!("(contains({args}, ' -p {package} ') || contains({args}, ' --workspace '))")
            }));
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
