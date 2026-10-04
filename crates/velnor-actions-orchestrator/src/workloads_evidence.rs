//! Native manifests cannot silently disappear into a successful empty plan.

use crate::OrchestratorError;
use velnor_actions_contract::config::{WorkloadConfig, WorkloadKind};
use velnor_actions_contract::{FileIndex, VelnorConfig};

/// Require an explicit reviewed native contract for each indexed manifest.
pub(super) fn qualify(config: &VelnorConfig, index: &FileIndex) -> Result<(), OrchestratorError> {
    for path in index.files() {
        let (root, name) = path.rsplit_once('/').unwrap_or((".", path));
        let kind = if path.ends_with(".xcodeproj/project.pbxproj") {
            WorkloadKind::NativeXcodeProjectCi
        } else if homebrew_source(path) {
            WorkloadKind::HomebrewAudit
        } else {
            match name {
                "Dockerfile" => WorkloadKind::DockerBuild,
                "package.json" => WorkloadKind::BunCi,
                "Package.swift" => WorkloadKind::SwiftTest,
                "build.gradle" | "build.gradle.kts" => WorkloadKind::GradleCheck,
                _ => continue,
            }
        };
        let declared = config.stacks.workloads.iter().any(|workload| {
            if kind == WorkloadKind::NativeXcodeProjectCi {
                return native_project_covers(workload, path);
            }
            if kind == WorkloadKind::HomebrewAudit {
                return homebrew_source_covered(workload, path);
            }
            if name == "Package.swift" && native_profile_covers(workload, path) {
                return true;
            }
            let matching_kind = workload.kind == kind
                || (name == "package.json" && workload.kind == WorkloadKind::NodeCi)
                || (matches!(name, "build.gradle" | "build.gradle.kts")
                    && workload.kind == WorkloadKind::GradleDatabaseCheck);
            matching_kind && workload.root.as_str() == root
        });
        if !declared {
            return Err(OrchestratorError::config(
                ".velnor/config.toml",
                "stacks.workloads",
                format!("native_obligation_undeclared:{path}"),
            ));
        }
    }
    Ok(())
}

/// A qualified native profile owns exactly its declared Swift manifest.
fn native_profile_covers(workload: &WorkloadConfig, manifest: &str) -> bool {
    if !matches!(
        workload.kind,
        WorkloadKind::NativeXcodeProjectCi | WorkloadKind::NativeSwiftPackageCi
    ) || workload.validate(".velnor/config.toml").is_err()
    {
        return false;
    }
    let Some(profile) = &workload.native_desktop else {
        return false;
    };
    native_path(
        workload.root.as_str(),
        &profile.native_root,
        "Package.swift",
    ) == manifest
}

/// A source project is covered only by its qualified exact Xcode descriptor.
fn native_project_covers(workload: &WorkloadConfig, manifest: &str) -> bool {
    if workload.kind != WorkloadKind::NativeXcodeProjectCi
        || workload.validate(".velnor/config.toml").is_err()
    {
        return false;
    }
    let Some(profile) = &workload.native_desktop else {
        return false;
    };
    let Some(apple) = &profile.apple else {
        return false;
    };
    native_path(
        workload.root.as_str(),
        &profile.native_root,
        &format!("{}/project.pbxproj", apple.project_path),
    ) == manifest
}

/// Dedicated source locations identify tap capability without inventing an audit.
fn homebrew_source(path: &str) -> bool {
    (path.starts_with("Formula/") || path.starts_with("Casks/")) && path.ends_with(".rb")
}

/// Syntax-only taps retain their exact source coverage; audits cover all targets.
fn homebrew_source_covered(workload: &WorkloadConfig, path: &str) -> bool {
    workload.validate(".velnor/config.toml").is_ok()
        && ((workload.kind == WorkloadKind::HomebrewAudit && workload.root.as_str() == ".")
            || (workload.kind == WorkloadKind::RubySyntax
                && workload.paths.iter().any(|covered| covered == path)))
}

/// Join only validated repository-relative components; `.` contributes no segment.
fn native_path(root: &str, native: &str, leaf: &str) -> String {
    [root, native, leaf]
        .into_iter()
        .filter(|part| *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
#[path = "workloads_evidence_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workloads_source_evidence_tests.rs"]
mod source_tests;
