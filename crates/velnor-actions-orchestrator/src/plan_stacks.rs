//! Plan `Detected stacks` section: crates, profiles, and evidence.

use std::collections::BTreeSet;

use velnor_actions_rust::DetectionStatus;

use crate::discover::{PlannedWorkspace, local_dep_names};
use crate::plan::push;
use crate::prepare::GenerationPreparation;

/// Detected stacks, crates, profiles, and evidence.
pub(crate) fn stacks_section(out: &mut String, prep: &GenerationPreparation) {
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
        profile_lines(out, workspace);
    }
}

/// Per-workspace profile provenance: drivers, sources, evidence, findings.
fn profile_lines(out: &mut String, workspace: &PlannedWorkspace) {
    let root = if workspace.record.workspace_root.is_empty() {
        "."
    } else {
        workspace.record.workspace_root.as_str()
    };
    let profile = &workspace.profile;
    push(
        out,
        &format!(
            "  Profile {root}: {} compile driver ({}), {} test runner ({})",
            profile.compile_driver.as_str(),
            profile.driver_source.as_str(),
            profile.test_runner.as_str(),
            profile.runner_source.as_str()
        ),
    );
    if profile.test_runner.as_str() == "cargo_nextest" {
        let config = profile
            .nextest_config
            .as_deref()
            .unwrap_or("no nextest config");
        push(
            out,
            &format!(
                "  Nextest profile {root}: {} ({config})",
                profile.nextest_profile.as_str()
            ),
        );
    }
    for evidence in &profile.evidence {
        push(
            out,
            &format!(
                "    evidence {}:{} {} [{}]",
                evidence.path,
                evidence.line,
                evidence.command_or_setting,
                evidence.strength.as_str()
            ),
        );
    }
    for finding in &workspace.findings {
        for sighting in &finding.evidence {
            push(
                out,
                &format!(
                    "    finding {} {}:{} {}: {}",
                    finding.code,
                    sighting.path,
                    sighting.line,
                    sighting.command_or_setting,
                    finding.message
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
