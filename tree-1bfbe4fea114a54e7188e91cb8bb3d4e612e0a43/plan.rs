//! Deterministic human-readable plan text (no JSON, YAML, or writes).

use std::collections::BTreeMap;

use velnor_actions_actionlint::ACTIONLINT_VERSION;
use velnor_actions_contract::{CRATE_JOB_ID_PREFIX, is_crate_job_id};
use velnor_actions_contract_config::{RunnerSelection, WorkflowPolicy};
use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_contract_workflow::{FRESHNESS_WORKFLOW_PATH, Job, RequiredCheckMigration};
use velnor_actions_rust::KIND_DISPLAY_WORDS;
use velnor_actions_workflow_renderer::action_pins;
use velnor_actions_workflow_renderer::release_tree::RELEASE_TREE_PATHS;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
use velnor_actions_workflow_tree::rendered::ACTIONLINT_PATH;

use crate::OrchestratorError;
use crate::finalized::finalized_jobs;
use crate::generate::render_staged_tree;
use crate::plan_stacks::stacks_section;
use crate::prepare::GenerationPreparation;

/// Render the concise deterministic `plan` report from a preparation,
/// after rendering the full tree in memory and discarding the bytes.
///
/// The job table, step counts, and action pins derive from the same
/// finalized jobs `generate` writes (validators included), so plan
/// and YAML agree by construction. Writes nothing.
///
/// # Errors
///
/// Returns render, actionlint, lock, or unsafe-path errors from the
/// discarded renderer pass.
pub fn plan_text_checked(prep: &GenerationPreparation) -> Result<String, OrchestratorError> {
    let _ = render_staged_tree(prep)?;
    let jobs = finalized_jobs(prep)?;
    Ok(plan_text(prep, &jobs))
}

/// Render the concise deterministic `plan` report from finalized jobs.
///
/// `jobs` must be [`finalized_jobs`] for `prep`: the attached IR plus
/// merged support, setup insertion, closures, and the final fan-in.
/// Passing pre-merge IR jobs would reintroduce the plan/YAML gaps
/// (missing validators, stale step counts, partial pins).
#[must_use]
pub fn plan_text(prep: &GenerationPreparation, jobs: &BTreeMap<String, Job>) -> String {
    let mut out = String::new();
    push(
        &mut out,
        &format!("Velnor Actions plan {}", env!("CARGO_PKG_VERSION")),
    );
    push(&mut out, &format!("Repository: {}", prep.root.display()));
    out.push('\n');
    stacks_section(&mut out, prep);
    workflow_section(&mut out, prep, jobs);
    migration_section(&mut out);
    recommendations_section(&mut out, prep);
    out
}

/// Append one line plus a newline.
pub(crate) fn push(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

/// Planned workflow files, jobs, matrix, runner, cache, and pins.
fn workflow_section(out: &mut String, prep: &GenerationPreparation, jobs: &BTreeMap<String, Job>) {
    out.push('\n');
    push(out, "Workflow to generate");
    push(out, &format!("  {ACTIONLINT_PATH}"));
    push(out, &format!("  {WORKFLOW_PATH}"));
    release_file_lines(out, prep);
    freshness_file_lines(out, prep);
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
    for (id, job) in jobs {
        push(out, &format!("    - {} ({} steps)", id, job.steps.len()));
    }
    crate_lines(out, prep, jobs);
    match crate_job_ids(jobs).len() {
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
    pin_lines(out, jobs);
    ineligible_lines(out, prep);
    feature_lines(out, prep);
    push(
        out,
        "  Pull-request execution narrows crate obligations through its event-time affected-work plan.",
    );
}

/// Every distinct action pin the finalized jobs embed, sorted.
///
/// The count and refs match the `uses:` lines `generate` writes, so a
/// new action in the YAML appears here without a plan-side allowlist.
fn pin_lines(out: &mut String, jobs: &BTreeMap<String, Job>) {
    let pins = action_pins(jobs);
    push(out, &format!("  Action pins: {}", pins.len()));
    for pin in &pins {
        push(out, &format!("    - {pin}"));
    }
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

/// Freshness workflow path, exactly when `generate` emits it.
fn freshness_file_lines(out: &mut String, prep: &GenerationPreparation) {
    if crate::freshness_emit::freshness_enabled(prep) {
        push(out, &format!("  {FRESHNESS_WORKFLOW_PATH}"));
    }
}

/// Finalized crate-job IDs: exactly the crate-group jobs.
///
/// Structural, never an exclusion list: plan, final, lint, validators,
/// candidate, and release carry other IDs, so merged support jobs can
/// never inflate the crate count.
fn crate_job_ids(jobs: &BTreeMap<String, Job>) -> Vec<&str> {
    jobs.keys()
        .filter(|id| is_crate_job_id(id))
        .map(String::as_str)
        .collect()
}

/// Crate-job count plus the per-obligation kind chain.
///
/// Counts finalized crate jobs, never task groups; otherwise the
/// static no-work line keeps plan and YAML in agreement.
fn crate_lines(out: &mut String, prep: &GenerationPreparation, jobs: &BTreeMap<String, Job>) {
    let crates = crate_job_ids(jobs);
    if crates.is_empty() {
        push(out, "    - no matrix entries (no-work workflow)");
        return;
    }
    let rust = crates
        .iter()
        .filter(|id| id.starts_with(CRATE_JOB_ID_PREFIX))
        .count();
    let tofu = crates.len() - rust;
    count_line(out, rust, "Rust crate job");
    count_line(out, tofu, "OpenTofu root job");
    let kinds = present_kinds(&prep.discovery.proposals);
    if !kinds.is_empty() {
        push(out, &format!("      Each: {}", kinds.join(" -> ")));
    }
    push(out, "      Entries:");
    for id in crates {
        push(out, &format!("        - {id}"));
    }
    let mut obligations: Vec<&str> = prep
        .discovery
        .proposals
        .iter()
        .filter(|task| crate::crate_jobs::is_runnable(task))
        .map(|task| task.task_id.as_str())
        .collect();
    obligations.sort_unstable();
    obligations.dedup();
    push(out, "      Obligations:");
    for id in obligations {
        push(out, &format!("        - {id}"));
    }
}

/// One per-stack count line; zero-count stacks stay silent.
///
/// Rust-only plans keep their exact historical line, so golden parity
/// holds; tofu adds its own line in fixed order below.
fn count_line(out: &mut String, count: usize, label: &str) {
    if count == 1 {
        push(out, &format!("    - 1 {label}"));
    } else if count > 1 {
        push(out, &format!("    - {count} {label}s"));
    }
}

/// Kind words in fixed order for the tasks present.
fn present_kinds(tasks: &[ProposedTask]) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    let words = KIND_DISPLAY_WORDS
        .into_iter()
        .chain(velnor_actions_tofu_core::KIND_DISPLAY_WORDS);
    for (kind, word) in words {
        if tasks
            .iter()
            .any(|task| task.task_kind == kind && crate::crate_jobs::is_runnable(task))
            && !kinds.contains(&word)
        {
            kinds.push(word);
        }
    }
    kinds
}

/// Structural critical path over the derived task proposals.
fn critical_path_lines(out: &mut String, prep: &GenerationPreparation) {
    let eligible: Vec<ProposedTask> = prep
        .discovery
        .proposals
        .iter()
        .filter(|task| !task.no_targets)
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

/// Cache layers derived from lock state plus selected drivers.
///
/// `Cargo sources` appears only when a lockfile exists to snapshot:
/// lockless repos emit no fetch or restore steps, so advertising the
/// layer would promise YAML that `generate` never writes.
fn cache_layers(prep: &GenerationPreparation) -> String {
    let mut layers = vec!["Mise tools"];
    if !crate::source_prep::lockful_roots(&prep.root, &prep.discovery.workspaces).is_empty() {
        layers.push("Cargo sources");
    }
    if prep.discovery.workspaces.iter().any(|workspace| {
        workspace.profile.compile_driver == velnor_actions_rust_core::CompileDriver::Mbx
    }) {
        layers.push("MBX compilation objects");
    }
    layers.join(", ")
}

/// Concise reasons for ineligible work.
fn ineligible_lines(out: &mut String, prep: &GenerationPreparation) {
    for task in &prep.discovery.proposals {
        if task.no_targets {
            push(
                out,
                &format!(
                    "  Ineligible: {} has no test targets; no test command emitted",
                    task.task_id
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

/// Required-check migration from the branded gate to `Required`.
///
/// Surfaces [`RequiredCheckMigration::velnor_to_ci`] so `plan` names
/// the same old/new checks the procedure doc gives admins. The final
/// step is an external branch-protection flip: the generator can only
/// print it, never perform it.
fn migration_section(out: &mut String) {
    let migration = RequiredCheckMigration::velnor_to_ci();
    let steps = migration.steps();
    out.push('\n');
    push(out, "Required-check migration");
    push(
        out,
        &format!(
            "  Old: {} (check: {})",
            migration.old_workflow, migration.old_check
        ),
    );
    push(
        out,
        &format!(
            "  New: {} (check: {})",
            migration.new_workflow, migration.new_check
        ),
    );
    for (index, step) in steps.iter().enumerate() {
        let external = if index + 1 == steps.len() {
            " [EXTERNAL: repository admin]"
        } else {
            ""
        };
        push(out, &format!("  {}. {step}{external}", index + 1));
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
