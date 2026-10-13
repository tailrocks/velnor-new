//! Generator identity resolution and baseline coverage application.

use velnor_actions_contract::{
    BaselineProof, BaselineStatus, ObligationDecision, Plan, PlanBaseline,
};
use velnor_actions_mise::catalog::lock::{load_text, parse_generator_lock};

use crate::merge::BaselineManifest;

/// Bootstrap lock resolving release generator identity for source builds.
const GENERATOR_LOCK_REL: &str = ".velnor/generator.lock";

/// Lookup-skipped reason for an unverifiable source-build generator.
pub(crate) const SOURCE_BUILD_REASON: &str = "generator_unverifiable_source_build";

/// True for generator SHAs that prove nothing: all-zero or empty.
///
/// An all-zero SHA is the source-build marker: no release binary stands
/// behind it, so baseline evidence bound to it is unverifiable.
pub(crate) fn is_source_build(sha: &str) -> bool {
    sha.is_empty() || (sha.len() == 64 && sha.bytes().all(|b| b == b'0'))
}

/// Fill an unverifiable generator SHA from the release lock when it pins
/// this exact version and target; otherwise keep the source-build marker.
///
/// Explicit caller-supplied SHAs win over the lock; any unreadable or
/// mismatched lock quietly leaves the marker, skipping live lookup.
pub(crate) fn resolve_generator_identity(plan: &mut Plan, root: &std::path::Path) {
    if !is_source_build(&plan.generator.sha256) {
        return;
    }
    let path = root.join(GENERATOR_LOCK_REL);
    if !path.is_file() {
        return;
    }
    let Ok(text) = load_text(&path) else {
        return;
    };
    let Ok(lock) = parse_generator_lock(&text) else {
        return;
    };
    let Some(target) = release_target(&plan.generator.target) else {
        return;
    };
    if lock.generator.version != plan.generator.version {
        return;
    }
    if let Some(record) = lock.binary_for_target(target) {
        plan.generator.sha256.clone_from(&record.sha256);
    }
}

/// Release triple for a generator target: triples pass through, else map.
///
/// Source builds record `{arch}-{os}`; release locks pin full triples.
fn release_target(target: &str) -> Option<&str> {
    if velnor_actions_contract::SUPPORTED_TARGETS.contains(&target) {
        return Some(target);
    }
    match target {
        "x86_64-linux" => Some("x86_64-unknown-linux-gnu"),
        "aarch64-macos" => Some("aarch64-apple-darwin"),
        "x86_64-macos" => Some("x86_64-apple-darwin"),
        _ => None,
    }
}

/// Mark covered obligations, prune the matrix, and record the baseline.
pub(crate) fn apply_coverage(
    plan: &mut Plan,
    manifest: &BaselineManifest,
    digest: &str,
    base: String,
) {
    for obligation in &mut plan.obligations {
        let hit = manifest.tasks.iter().find(|task| {
            task.task_id == obligation.task_id
                && task.task_digest == obligation.task_digest
                && task.input_digest == obligation.input_digest
        });
        if let Some(task) = hit {
            obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
            obligation.reason = String::from("covered_by_trusted_baseline");
            obligation.baseline_proof = Some(BaselineProof {
                source_commit: manifest.source_commit.clone(),
                run_id: task.proof_run_id,
                artifact_id: manifest.artifact_id,
                artifact_name: manifest.artifact_name.clone(),
                manifest_digest: digest.to_owned(),
            });
        } else {
            plan.warnings
                .push(format!("baseline_miss:{}:no_entry", obligation.task_id));
        }
    }
    plan.matrix.include.retain(|entry| {
        plan.obligations
            .iter()
            .any(|ob| ob.task_id == entry.task_id && ob.decision == ObligationDecision::Execute)
    });
    for package in &mut plan.packages {
        package.selected = plan.obligations.iter().any(|ob| {
            ob.decision == ObligationDecision::Execute && package.tasks.contains(&ob.task_id)
        });
    }
    plan.baseline = PlanBaseline {
        status: BaselineStatus::Used,
        base_commit: Some(base),
        run_id: Some(manifest.run_id),
        artifact_id: Some(manifest.artifact_id),
        artifact_name: Some(manifest.artifact_name.clone()),
        manifest_digest: Some(digest.to_owned()),
        reason: None,
    };
}
