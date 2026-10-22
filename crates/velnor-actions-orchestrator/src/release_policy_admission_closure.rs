//! Path-closure and version checks for preview source admissions.

use std::collections::BTreeSet;

use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{PackageReleasePlan, preview_production::ReleaseProductionContext};

use super::source_files;

pub(super) fn admission_paths(
    files: &[source_files::SourceFileEntry],
) -> Result<BTreeSet<String>, OrchestratorError> {
    let paths = files
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    if paths.len() != files.len() {
        return Err(internal("release_policy_source_closure_duplicate"));
    }
    Ok(paths)
}

pub(super) fn admission_covers_required_paths(
    required: &BTreeSet<String>,
    retained: &BTreeSet<String>,
) -> bool {
    required.is_subset(retained)
}

pub(super) fn admitted_preview_version_matches(
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
) -> bool {
    velnor_actions_contract::preview_identity::PreviewIdentity::derive(
        &plan.base_version,
        &context.source_sha,
        context.commit_count,
    )
    .is_ok_and(|identity| identity.version == context.version)
}
