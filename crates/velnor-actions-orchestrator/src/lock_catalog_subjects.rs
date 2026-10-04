//! Host-qualified emitted catalog installation audit subjects.

use std::collections::BTreeMap;
use velnor_actions_contract::{Job, tool_target_for_runner_label};
use velnor_actions_mise::catalog::qualification::DistributionHost;
use velnor_actions_mise::toolfiles::lockfile::{
    InstallSubject, mise_platform_for_target, subject_for_install_spec,
};
use velnor_actions_mise::{PREPARE_PINNED_TOOLS_STEP, ToolCatalog};

pub(super) type Subjects = BTreeMap<String, Vec<InstallSubject>>;

pub(super) fn collect(
    id: &str,
    job: &Job,
    catalog: &ToolCatalog,
    groups: &mut Subjects,
    blocking: &mut Vec<String>,
) {
    for step in &job.steps {
        if step.name != PREPARE_PINNED_TOOLS_STEP {
            continue;
        }
        let Some(specs) = super::prepare::prepare_specs(id, step, blocking) else {
            continue;
        };
        if specs.is_empty() {
            blocking.push(super::bare_install(id));
            continue;
        }
        let target = tool_target_for_runner_label(&job.runs_on);
        let (Some(host), Some(platform)) = (
            target.and_then(DistributionHost::for_target),
            target.and_then(mise_platform_for_target),
        ) else {
            blocking.push(format!("unauditable_install_host:{id}:{}", job.runs_on));
            continue;
        };
        let subjects = groups.entry(platform.to_owned()).or_default();
        for spec in specs {
            match subject_for_install_spec(&spec, catalog, host) {
                Ok(Some(subject)) => subjects.push(subject),
                Ok(None) => blocking.push(format!("unauditable_install_spec:{spec}")),
                Err(error) => blocking.push(format!(
                    "unauditable_install_qualification:{id}:{spec}:{error}"
                )),
            }
        }
    }
}

/// Coverage is bound to each installation's own platform, including duplicates.
pub(super) fn audit(
    lock_text: Option<&str>,
    groups: Subjects,
    mut blocking: Vec<String>,
) -> super::LockAuditOutcome {
    let mut recommendations = Vec::new();
    for (platform, mut subjects) in groups {
        subjects.sort_by(|left, right| left.display.cmp(&right.display));
        subjects.dedup();
        let outcome = super::audit_against_lock(lock_text, &subjects, &platform, Vec::new());
        blocking.extend(outcome.blocking);
        if let Some(recommendation) = outcome.recommendation {
            recommendations.push(format!("{platform}:{recommendation}"));
        }
    }
    super::LockAuditOutcome {
        recommendation: (!recommendations.is_empty()).then(|| recommendations.join("; ")),
        blocking,
    }
}
