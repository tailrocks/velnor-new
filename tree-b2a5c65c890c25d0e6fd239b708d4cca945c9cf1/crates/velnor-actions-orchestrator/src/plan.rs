//! Deterministic human-readable plan text (no JSON, YAML, or writes).

use std::collections::BTreeSet;

use velnor_actions_actionlint::ACTIONLINT_VERSION;
use velnor_actions_contract::{RunnerSelection, WorkflowPolicy};
use velnor_actions_rust::{DetectionStatus, TaskGroup, TaskKind};
use velnor_actions_workflow_renderer::render::{ACTIONLINT_PATH, WORKFLOW_PATH};

use crate::discover::local_dep_names;
use crate::prepare::GenerationPreparation;
use crate::workflow::CHECKOUT_USES;

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
fn push(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

/// Detected stacks, crates, profiles, and evidence.
fn stacks_section(out: &mut String, prep: &GenerationPreparation) {
    push(out, "Detected stacks");
    let ignored = prep
        .discovery
        .statuses
        .iter()
        .any(|status| matches!(status, DetectionStatus::Ignored { .. }));
    if ignored {
        push(out, "  Rust: ignored (config stacks.ignore)");
        for status in &prep.discovery.statuses {
            if let DetectionStatus::Ignored { project, reason } = status {
                push(
                    out,
                    &format!("    - {}: ignored ({})", project.manifest, reason),
                );
            }
        }
    } else {
        push(out, "  Rust: selected");
    }
    let crates = sorted_crates(prep);
    push(out, &format!("  Workspace crates: {}", crates.len()));
    for (name, manifest, detail) in &crates {
        push(out, &format!("    - {name} ({manifest}) [{detail}]"));
    }
    for workspace in &prep.discovery.workspaces {
        let root = if workspace.record.workspace_root.is_empty() {
            "."
        } else {
            workspace.record.workspace_root.as_str()
        };
        push(
            out,
            &format!(
                "  Profile {root}: {} compile driver, {} test runner",
                workspace.profile.compile_driver.as_str(),
                workspace.profile.test_runner.as_str()
            ),
        );
        for evidence in &workspace.profile.evidence {
            push(
                out,
                &format!(
                    "    evidence {}:{} {}",
                    evidence.path, evidence.line, evidence.command_or_setting
                ),
            );
        }
    }
}

/// Crates sorted by name then manifest with kind and dependency detail.
fn sorted_crates(prep: &GenerationPreparation) -> Vec<(String, String, String)> {
    let mut crates = Vec::new();
    for workspace in &prep.discovery.workspaces {
        for package in &workspace.record.packages {
            if !package.in_workspace || package.external {
                continue;
            }
            let mut kinds: BTreeSet<&str> = package
                .targets
                .iter()
                .map(|target| target.kind.as_str())
                .collect();
            kinds.remove("custom-build");
            if kinds.is_empty() {
                kinds.insert("lib");
            }
            let kinds = kinds.into_iter().collect::<Vec<_>>().join(", ");
            let deps = local_dep_names(&workspace.record, &package.id).join(", ");
            let detail = if deps.is_empty() {
                kinds
            } else {
                format!("{kinds}; depends on {deps}")
            };
            crates.push((package.name.clone(), package.manifest.clone(), detail));
        }
    }
    crates.sort();
    crates
}

/// Planned workflow files, jobs, matrix, runner, cache, and pins.
fn workflow_section(out: &mut String, prep: &GenerationPreparation) {
    out.push('\n');
    push(out, "Workflow to generate");
    push(out, &format!("  {ACTIONLINT_PATH}"));
    push(out, &format!("  {WORKFLOW_PATH}"));
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
    push(out, "    - plan and formatting");
    matrix_lines(out, prep);
    push(out, "    - final required result");
    push(
        out,
        &format!(
            "  Parallel: up to {} matrix entries; independent crate entries",
            prep.config.workflow.max_parallel_jobs
        ),
    );
    push(out, &format!("  Cache layers: {}", cache_layers(prep)));
    push(out, &format!("  Actionlint: {ACTIONLINT_VERSION} pinned"));
    push(out, &format!("  Action pins: {CHECKOUT_USES}"));
    ineligible_lines(out, prep);
    push(
        out,
        "  Pull-request execution narrows crate obligations through its event-time affected-work plan.",
    );
}

/// Matrix entry count plus the per-entry kind chain.
fn matrix_lines(out: &mut String, prep: &GenerationPreparation) {
    let runnable = prep
        .discovery
        .task_groups
        .iter()
        .filter(|group| !group.no_test_targets)
        .count();
    if runnable == 0 {
        push(out, "    - no matrix entries (no-work workflow)");
        return;
    }
    push(out, &format!("    - Rust crate matrix: {runnable} entries"));
    let kinds = present_kinds(&prep.discovery.task_groups);
    if !kinds.is_empty() {
        push(out, &format!("      Each: {}", kinds.join(" -> ")));
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
