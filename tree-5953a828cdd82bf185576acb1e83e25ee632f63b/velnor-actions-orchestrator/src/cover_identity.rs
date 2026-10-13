//! Generator identity resolution and baseline coverage application.

use velnor_actions_contract::{
    BaselineProof, BaselineStatus, ObligationDecision, Plan, PlanBaseline,
};
use velnor_actions_mise::catalog::lock::{load_text, parse_generator_lock};

use crate::extension_schemas::coverage_schema_known;
use crate::external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, external_data_kind, may_skip_external_data,
};
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
        let Some(task) = hit else {
            plan.warnings
                .push(format!("baseline_miss:{}:no_entry", obligation.task_id));
            continue;
        };
        if !coverage_schema_known(&obligation.task_id) {
            plan.warnings.push(format!(
                "baseline_miss:{}:unknown_extension_schema",
                obligation.task_id
            ));
            continue;
        }
        if external_data_kind(&obligation.task_id).is_some()
            && !may_skip_external_data(
                true,
                task.external_data.as_ref(),
                DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS,
            )
        {
            plan.warnings.push(format!(
                "baseline_miss:{}:external_data_rerun",
                obligation.task_id
            ));
            continue;
        }
        obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
        obligation.reason = String::from("covered_by_trusted_baseline");
        obligation.baseline_proof = Some(BaselineProof {
            source_commit: manifest.source_commit.clone(),
            run_id: task.proof_run_id,
            artifact_id: manifest.artifact_id,
            artifact_name: manifest.artifact_name.clone(),
            manifest_digest: digest.to_owned(),
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_data::ExternalDataFreshness;
    use crate::merge::{BaselineManifest, BaselineTaskEntry};
    use velnor_actions_contract::{
        ObligationDecision, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, RunnerSelection,
        Trust, WorkflowEvent, digest_b3,
    };

    /// Plan carrying one execute obligation per `task_ids`.
    fn plan_with(task_ids: &[&str]) -> Plan {
        let digest = digest_b3(b"digest");
        Plan {
            schema: 1,
            run_key: "local".to_owned(),
            plan_id: "plan-local".to_owned(),
            base: None,
            head: "head".to_owned(),
            event: WorkflowEvent::PullRequest,
            runner: PlanRunner {
                label: "ubuntu-26.04".to_owned(),
                selection: RunnerSelection::LatestDefault,
            },
            trust: Trust::Pr,
            baseline: PlanBaseline {
                status: BaselineStatus::Unavailable,
                base_commit: None,
                run_id: None,
                artifact_id: None,
                artifact_name: None,
                manifest_digest: None,
                reason: None,
            },
            generator: PlanGenerator {
                version: "0.1.0".to_owned(),
                target: "x86_64-linux".to_owned(),
                sha256: "0".repeat(64),
            },
            packages: Vec::new(),
            obligations: task_ids
                .iter()
                .map(|id| PlanObligation {
                    task_id: (*id).to_owned(),
                    decision: ObligationDecision::Execute,
                    reason: "selected".to_owned(),
                    task_digest: digest.clone(),
                    input_digest: digest.clone(),
                    baseline_proof: None,
                })
                .collect(),
            matrix: PlanMatrix {
                include: Vec::new(),
            },
            task_ids: task_ids.iter().map(|id| (*id).to_owned()).collect(),
            warnings: Vec::new(),
        }
    }

    /// Manifest binding every `task_ids` entry, with advisory freshness.
    fn manifest_with(
        task_ids: &[&str],
        external_data: Option<&ExternalDataFreshness>,
    ) -> BaselineManifest {
        let digest = digest_b3(b"digest");
        BaselineManifest {
            schema: 1,
            repository_id: digest.clone(),
            source_commit: "a".repeat(40),
            ref_: "refs/heads/testmain".to_owned(),
            event: "push".to_owned(),
            workflow_ref: "o/r/.github/workflows/velnor.yml@refs/heads/testmain".to_owned(),
            run_id: 7,
            run_attempt: 1,
            final_status: "passed".to_owned(),
            generator_version: "0.1.0".to_owned(),
            generator_sha256: "1".repeat(64),
            compatibility_id: digest.clone(),
            artifact_id: 9,
            artifact_name: "velnor-baseline".to_owned(),
            expires_at_unix: None,
            tasks: task_ids
                .iter()
                .map(|id| BaselineTaskEntry {
                    task_id: (*id).to_owned(),
                    task_digest: digest.clone(),
                    input_digest: digest.clone(),
                    proof_run_id: 7,
                    observed_run_id: 7,
                    external_data: external_data.cloned(),
                })
                .collect(),
        }
    }

    /// Fresh advisory proof over a fixed identity.
    fn fresh_proof() -> ExternalDataFreshness {
        ExternalDataFreshness {
            source: "advisory-db".to_owned(),
            identity: digest_b3(b"snapshot"),
            age_secs: 60,
        }
    }

    #[test]
    fn unknown_schema_and_stale_external_data_stay_execute() {
        let rust = "stack/rust/root/clippy/default";
        let unknown = "stack/unknown/root/test/default";
        let advisory = "stack/rust/root/advisory/default";
        let mut plan = plan_with(&[rust, unknown, advisory]);
        let manifest = manifest_with(&[rust, unknown, advisory], None);
        apply_coverage(&mut plan, &manifest, &digest_b3(b"m"), "base".to_owned());
        let decision = |id: &str| {
            plan.obligations
                .iter()
                .find(|ob| ob.task_id == id)
                .map(|ob| ob.decision)
        };
        assert_eq!(
            decision(rust),
            Some(ObligationDecision::CoveredByTrustedBaseline)
        );
        assert_eq!(decision(unknown), Some(ObligationDecision::Execute));
        assert_eq!(decision(advisory), Some(ObligationDecision::Execute));
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.contains("unknown_extension_schema"))
        );
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.contains("external_data_rerun"))
        );
    }

    #[test]
    fn fresh_external_data_covers_advisory() {
        let advisory = "stack/rust/root/advisory/default";
        let mut plan = plan_with(&[advisory]);
        let proof = fresh_proof();
        let manifest = manifest_with(&[advisory], Some(&proof));
        apply_coverage(&mut plan, &manifest, &digest_b3(b"m"), "base".to_owned());
        assert_eq!(
            plan.obligations[0].decision,
            ObligationDecision::CoveredByTrustedBaseline
        );
        assert!(plan.obligations[0].baseline_proof.is_some());
    }
}
