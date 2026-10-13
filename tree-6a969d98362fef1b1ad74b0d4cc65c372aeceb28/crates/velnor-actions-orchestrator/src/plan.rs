//! Deterministic human-readable plan text (no JSON, YAML, or writes).

use velnor_actions_actionlint::ACTIONLINT_VERSION;
use velnor_actions_contract::{RunnerSelection, WorkflowPolicy};
use velnor_actions_rust::{TaskGroup, TaskKind};
use velnor_actions_workflow_renderer::release_tree::RELEASE_TREE_PATHS;
use velnor_actions_workflow_renderer::render::{
    ACTIONLINT_PATH, FINAL_JOB_ID, PLAN_JOB_ID, WORKFLOW_PATH,
};

use crate::OrchestratorError;
use crate::generate::render_staged_tree;
use crate::plan_stacks::stacks_section;
use crate::prepare::GenerationPreparation;
use crate::workflow::{CHECKOUT_USES, LINT_JOB_ID};

/// Render the concise deterministic `plan` report from a preparation,
/// after rendering the full tree in memory and discarding the bytes.
///
/// Fails when generation-time rendering would fail, keeping `plan` on
/// the same renderer output `generate` writes. Writes nothing.
///
/// # Errors
///
/// Returns render, actionlint, lock, or unsafe-path errors from the
/// discarded renderer pass.
pub fn plan_text_checked(prep: &GenerationPreparation) -> Result<String, OrchestratorError> {
    let _ = render_staged_tree(prep)?;
    Ok(plan_text(prep))
}

/// Render the concise deterministic `plan` report from a preparation.
#[must_use]
pub fn plan_text(prep: &GenerationPreparation) -> String {
    let mut out = String::new();
    push(
        &mut out,
        &format!("Velnor Actions plan {}", env!("CARGO_PKG_VERSION")),
    );
    push(&mut out, &format!("Repository: {}", prep.root.display()));
    out.push('\n');
    stacks_section(&mut out, prep);
    workflow_section(&mut out, prep);
    recommendations_section(&mut out, prep);
    out
}

/// Append one line plus a newline.
pub(crate) fn push(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

/// Planned workflow files, jobs, matrix, runner, cache, and pins.
fn workflow_section(out: &mut String, prep: &GenerationPreparation) {
    out.push('\n');
    push(out, "Workflow to generate");
    push(out, &format!("  {ACTIONLINT_PATH}"));
    push(out, &format!("  {WORKFLOW_PATH}"));
    release_file_lines(out, prep);
    push(
        out,
        &format!(
            "  Workflow: {} ({})",
            prep.config.workflow.name,
            policy_name(prep.config.workflow.policy)
        ),
    );
    push(out, &format!("  Push branch: {}", prep.default_branch));
    let provenance = match prep.runner_selection {
        RunnerSelection::LatestDefault => "latest pinned default",
        RunnerSelection::ConfigOverride => "config override",
    };
    push(
        out,
        &format!("  Runner: {} ({provenance})", prep.runner_label),
    );
    push(out, "  Jobs:");
    for (id, job) in &prep.workflow.ir.jobs {
        push(out, &format!("    - {} ({} steps)", id, job.steps.len()));
    }
    crate_lines(out, prep);
    match crate_job_ids(prep).len() {
        0 => push(out, "  Parallel: single job; no matrix fan-out"),
        1 => push(
            out,
            "  Parallel: 1 independent crate job; no matrix fan-out",
        ),
        count => push(
            out,
            &format!("  Parallel: {count} independent crate jobs; no matrix fan-out"),
        ),
    }
    critical_path_lines(out, prep);
    clippy_lines(out, prep);
    push(out, &format!("  Cache layers: {}", cache_layers(prep)));
    push(out, &format!("  Actionlint: {ACTIONLINT_VERSION} pinned"));
    push(out, &format!("  Action pins: {CHECKOUT_USES}"));
    ineligible_lines(out, prep);
    feature_lines(out, prep);
    push(
        out,
        "  Pull-request execution narrows crate obligations through its event-time affected-work plan.",
    );
}

/// Release-owned tree paths, exactly what `generate` emits when enabled.
///
/// The checked plan entrypoint surfaces config errors from its discarded
/// render pass first, so an error here means nothing is emitted.
fn release_file_lines(out: &mut String, prep: &GenerationPreparation) {
    let enabled = crate::release_emit::enabled_release(prep)
        .ok()
        .flatten()
        .is_some();
    if enabled {
        for path in RELEASE_TREE_PATHS {
            push(out, &format!("  {path}"));
        }
    }
}

/// IR crate-job IDs: every finalized job except plan, final, and lint.
fn crate_job_ids(prep: &GenerationPreparation) -> Vec<&str> {
    prep.workflow
        .ir
        .jobs
        .keys()
        .filter(|id| {
            id.as_str() != PLAN_JOB_ID && id.as_str() != FINAL_JOB_ID && id.as_str() != LINT_JOB_ID
        })
        .map(String::as_str)
        .collect()
}

/// Crate-job count plus the per-obligation kind chain.
///
/// Counts finalized IR crate jobs, never task groups; otherwise the
/// static no-work line keeps plan and YAML in agreement.
fn crate_lines(out: &mut String, prep: &GenerationPreparation) {
    let crates = crate_job_ids(prep);
    if crates.is_empty() {
        push(out, "    - no matrix entries (no-work workflow)");
        return;
    }
    if crates.len() == 1 {
        push(out, "    - 1 Rust crate job");
    } else {
        push(out, &format!("    - {} Rust crate jobs", crates.len()));
    }
    let kinds = present_kinds(&prep.discovery.task_groups);
    if !kinds.is_empty() {
        push(out, &format!("      Each: {}", kinds.join(" -> ")));
    }
    push(out, "      Entries:");
    for id in crates {
        push(out, &format!("        - {id}"));
    }
    let mut obligations: Vec<&str> = prep
        .discovery
        .task_groups
        .iter()
        .filter(|group| !group.no_test_targets)
        .map(|group| group.task_id.as_str())
        .collect();
    obligations.sort_unstable();
    obligations.dedup();
    push(out, "      Obligations:");
    for id in obligations {
        push(out, &format!("        - {id}"));
    }
}

/// Kind words in fixed order for the groups present.
fn present_kinds(groups: &[TaskGroup]) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    for (kind, word) in [
        (TaskKind::Clippy, "Clippy"),
        (TaskKind::Build, "build"),
        (TaskKind::Test, "run tests"),
        (TaskKind::Nextest, "run tests"),
        (TaskKind::Doctest, "doctests"),
        (TaskKind::Doc, "doc build"),
        (TaskKind::Fmt, "format check"),
    ] {
        if groups
            .iter()
            .any(|group| group.kind == kind && !group.no_test_targets)
            && !kinds.contains(&word)
        {
            kinds.push(word);
        }
    }
    kinds
}

/// Structural critical path over the derived task groups.
fn critical_path_lines(out: &mut String, prep: &GenerationPreparation) {
    let eligible: Vec<TaskGroup> = prep
        .discovery
        .task_groups
        .iter()
        .filter(|group| !group.no_test_targets)
        .cloned()
        .collect();
    let path = crate::critical_path::critical_path_structural(&eligible);
    push(
        out,
        &format!("  {}", crate::critical_path::critical_path_line(&path)),
    );
}

/// Barrier-separated Clippy memory schedule.
fn clippy_lines(out: &mut String, prep: &GenerationPreparation) {
    let memory = &prep.discovery.clippy_memory;
    push(
        out,
        &format!(
            "  Clippy memory groups: {}; barriers: {}",
            memory.groups.len(),
            memory.barriers
        ),
    );
}

/// Cache layers derived from selected drivers.
fn cache_layers(prep: &GenerationPreparation) -> String {
    let mut layers = vec!["Mise tools", "Cargo sources"];
    if prep
        .discovery
        .workspaces
        .iter()
        .any(|workspace| workspace.profile.compile_driver.as_str() == "mbx")
    {
        layers.push("MBX compilation objects");
    }
    layers.join(", ")
}

/// Concise reasons for ineligible work.
fn ineligible_lines(out: &mut String, prep: &GenerationPreparation) {
    for group in &prep.discovery.task_groups {
        if group.no_test_targets {
            push(
                out,
                &format!(
                    "  Ineligible: {} has no test targets; no test command emitted",
                    group.task_id
                ),
            );
        }
    }
}

/// Per-crate feature narrowing, never silent.
fn feature_lines(out: &mut String, prep: &GenerationPreparation) {
    for fallback in &prep.discovery.feature_fallbacks {
        let requested = fallback.requested.join(",");
        if fallback.used_defaults() {
            push(
                out,
                &format!(
                    "  Features: {} [{}] declares none of [{requested}]; using default features",
                    fallback.package_name, fallback.configuration
                ),
            );
        } else {
            let applied = fallback.applied.join(",");
            push(
                out,
                &format!(
                    "  Features: {} [{}] requested [{requested}]; applied [{applied}]",
                    fallback.package_name, fallback.configuration
                ),
            );
        }
    }
}

/// Recommendations shared with `generate`.
fn recommendations_section(out: &mut String, prep: &GenerationPreparation) {
    out.push('\n');
    push(out, "Recommendations");
    if prep.discovery.recommendations.is_empty() {
        push(out, "  (none)");
    }
    for recommendation in &prep.discovery.recommendations {
        push(out, &format!("  {recommendation}"));
    }
}

/// Policy selector in config spelling.
fn policy_name(policy: WorkflowPolicy) -> &'static str {
    match policy {
        WorkflowPolicy::ConsumerV1 => "consumer-v1",
        WorkflowPolicy::VelnorRepositoryV1 => "velnor-repository-v1",
    }
}
