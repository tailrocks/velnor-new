// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Gate, GateNeed, Job, PlanFrom, RootSlug, Visibility, When};

const SETUP_DONE: &str = "steps.restored.outcome == 'success'";
pub(crate) const PLAN_STEP: &str = "plan";
pub(crate) const CACHE_DEBUG_ON: &str = "vars.VELNOR_CACHE_DEBUG == '1'";
const NOT_CANCELLED: &str = "!cancelled()";
const NOT_DRAFT: &str = "github.event.pull_request.draft != true";
const SCHEDULE: &str = "schedule";
const PUSH: &str = "push";
pub(crate) const TOOLS_STEP: &str = "tools";
pub(crate) const BASE_STEP: &str = "base";
const SAVING_EVENTS: &str = "(github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";
const ON_DEFAULT_BRANCH: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";
const ACTIVE_EVENT: &str = "(github.event_name != 'pull_request' || (startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed'))";

pub(crate) fn job_condition(job: &Job) -> String {
    let predicate = condition(
        job.when,
        job.gate.as_ref(),
        false,
        job.skip_on_draft,
        &visibility_part(job.visibility)
            .into_iter()
            .collect::<Vec<_>>(),
    );
    match predicate {
        Some(predicate) => format!(
            "${{{{ {ACTIVE_EVENT} && ({}) }}}}",
            predicate.trim_start_matches("${{ ").trim_end_matches(" }}")
        ),
        None => format!("${{{{ {ACTIVE_EVENT} }}}}"),
    }
}

pub(crate) fn sources_step(slug: &RootSlug) -> String {
    format!("sources_{slug}")
}

pub(crate) fn output(slug: &RootSlug, need: GateNeed) -> String {
    if need.is_global() {
        format!("rust_{}", need_name(need))
    } else {
        format!("rust_{slug}_{}", need_name(need))
    }
}

pub(crate) fn plan_source(from: &PlanFrom) -> String {
    match from {
        PlanFrom::ThisJob => format!("steps.{PLAN_STEP}.outputs"),
        PlanFrom::Job(job) => format!("needs.{job}.outputs"),
    }
}

fn need_name(need: GateNeed) -> &'static str {
    match need {
        GateNeed::Selected => "mode",
        GateNeed::Doctests => "doctests",
        GateNeed::DoctestsKnown => "doctests_known",
        GateNeed::Docs => "docs",
        GateNeed::TestFilter => "test_filter",
        GateNeed::PackageArgs => "package_args",
        GateNeed::Deny => "deny",
        GateNeed::Machete => "machete",
        GateNeed::Policy => "policy",
    }
}

pub(crate) const PRIVATE_REPOSITORY: &str = "github.event.repository.private == true";
const PUBLIC_REPOSITORY: &str = "github.event.repository.private != true";

pub(crate) fn visibility_part(visibility: Visibility) -> Option<String> {
    match visibility {
        Visibility::Any => None,
        Visibility::PrivateOnly => Some(PRIVATE_REPOSITORY.to_owned()),
        Visibility::PublicOnly => Some(PUBLIC_REPOSITORY.to_owned()),
    }
}

pub(crate) fn condition(
    when: When,
    gate: Option<&Gate>,
    after_setup: bool,
    skip_draft: bool,
    extra: &[String],
) -> Option<String> {
    let mut parts = Vec::new();
    match when {
        When::Success => {}
        When::Finalizer => parts.push(NOT_CANCELLED.to_owned()),
        When::NotCancelled => {
            parts.push(NOT_CANCELLED.to_owned());
            if after_setup {
                parts.push(SETUP_DONE.to_owned());
            }
        }
        When::CacheDebug => {
            parts.push(NOT_CANCELLED.to_owned());
            parts.push(CACHE_DEBUG_ON.to_owned());
            if after_setup {
                parts.push(SETUP_DONE.to_owned());
            }
        }
        When::Scheduled => parts.push(format!("github.event_name == '{SCHEDULE}'")),
        When::ScheduledOrPush => parts.push(format!(
            "(github.event_name == '{SCHEDULE}' || github.event_name == '{PUSH}')"
        )),
        When::SaveBase | When::SaveBaseDecided => {
            parts.push(NOT_CANCELLED.to_owned());
            if after_setup {
                parts.push(SETUP_DONE.to_owned());
            }
            parts.push(ON_DEFAULT_BRANCH.to_owned());
            parts.push(SAVING_EVENTS.to_owned());
            if when == When::SaveBaseDecided {
                parts.push(format!("steps.{BASE_STEP}.outputs.save == 'true'"));
            }
        }
        When::SavingRun => {
            parts.push(ON_DEFAULT_BRANCH.to_owned());
            parts.push(SAVING_EVENTS.to_owned());
        }
        When::SaveToolsOnDefaultBranch => {
            parts.push(ON_DEFAULT_BRANCH.to_owned());
            parts.push(format!("steps.{TOOLS_STEP}.outputs.cache-hit != 'true'"));
        }
    }
    if skip_draft {
        parts.push(NOT_DRAFT.to_owned());
    }
    parts.extend(extra.iter().cloned());
    if let Some(gate) = gate {
        parts.extend(gate_parts(gate));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("${{{{ {} }}}}", parts.join(" && ")))
    }
}

fn gate_parts(gate: &Gate) -> Vec<String> {
    let source = plan_source(&gate.from);
    let mut parts = match &gate.from {
        PlanFrom::ThisJob => vec![format!("steps.{PLAN_STEP}.outcome == 'success'")],
        PlanFrom::Job(_) => Vec::new(),
    };
    let read = |need| format!("{source}.{}", output(&gate.slug, need));
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
        GateNeed::TestFilter | GateNeed::PackageArgs | GateNeed::DoctestsKnown => {
            parts.push(format!("{} != ''", read(gate.need)));
        }
    }
    parts
}
