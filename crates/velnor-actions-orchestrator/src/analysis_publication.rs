//! Qualified fresh Cargo analysis staged outside run evidence artifacts.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

#[path = "runtime_trust.rs"]
mod runtime_trust;

use crate::OrchestratorError;
use crate::analysis_inventory::{
    AnalysisIdentity, AnalysisSource, build_payload, resolution_inputs_digest,
};
use crate::internal::internal;
use crate::prepare::GenerationPreparation;

/// Runner facts captured together at the request-writing boundary.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisPublicationContext {
    repository: String,
    head: String,
    workflow_sha: String,
    workflow_ref: String,
    branch: String,
    default_branch: String,
    run_id: u64,
    run_attempt: u32,
    protected: bool,
    event: String,
}

/// Authoritative outputs from the producer; payload bytes never enter plan JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisPublicationOutputs {
    /// Immutable name shared with authenticated retrieval.
    pub artifact_name: String,
    /// Single payload file under a separate runner-owned staging directory.
    pub artifact_path: PathBuf,
}

/// Optional publication cannot weaken or prevent fresh required planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AnalysisPublicationAttempt {
    /// This run or discovery has no publishable fresh analysis.
    NotRequested,
    /// A complete immutable payload was staged successfully.
    Published(AnalysisPublicationOutputs),
    /// Proof or staging could not establish a reusable artifact.
    Unavailable { reason: String },
}

impl AnalysisPublicationContext {
    /// Capture only push runs with complete runner identity; other runs publish nothing.
    pub(crate) fn capture(payload: &serde_json::Value) -> Option<Self> {
        let read = |key| std::env::var(key).ok().filter(|value| !value.is_empty());
        Some(Self {
            repository: crate::origin::validate_repository_slug(&read("GITHUB_REPOSITORY")?)?,
            head: read("GITHUB_SHA")?,
            workflow_sha: read("GITHUB_WORKFLOW_SHA")?,
            workflow_ref: read("GITHUB_WORKFLOW_REF")?,
            branch: read("GITHUB_REF_NAME")?,
            default_branch: payload["repository"]["default_branch"].as_str()?.to_owned(),
            run_id: read("GITHUB_RUN_ID")?.parse().ok()?,
            run_attempt: read("GITHUB_RUN_ATTEMPT")?.parse().ok()?,
            protected: read("GITHUB_REF_PROTECTED").as_deref() == Some("true"),
            event: read("GITHUB_EVENT_NAME")?,
        })
    }

    pub(crate) fn qualifies(&self, prep: &GenerationPreparation, head: &str) -> bool {
        self.matches_source(head, &prep.default_branch)
            && crate::cover_baseline::provenance_check::repository_slug_from_origin(&prep.root)
                .as_deref()
                == Some(self.repository.as_str())
    }

    fn matches_source(&self, head: &str, branch: &str) -> bool {
        let workflow = velnor_actions_workflow_renderer::WORKFLOW_PATH;
        let suffix = format!("/{workflow}@refs/heads/{}", self.branch);
        let workflow_repository = self
            .workflow_ref
            .strip_suffix(&suffix)
            .and_then(crate::origin::validate_repository_slug);
        self.event == "push"
            && self.protected
            && self.run_id > 0
            && self.run_attempt > 0
            && self.head == head
            && self.workflow_sha == head
            && self.branch == branch
            && self.default_branch == branch
            && workflow_repository.as_deref() == Some(self.repository.as_str())
            && crate::origin::validate_repository_slug(&self.repository).as_deref()
                == Some(self.repository.as_str())
    }
}

/// Stage only complete qualified inventories produced through fresh Cargo.
/// No downloaded/local DTO can enter this path through an inventory capability.
pub(crate) fn stage_analysis(
    prep: &GenerationPreparation,
    head: &str,
    context: Option<&AnalysisPublicationContext>,
    runner_temp: &Path,
) -> Result<AnalysisPublicationAttempt, OrchestratorError> {
    publication_attempt(try_stage_analysis(prep, head, context, runner_temp))
}

fn publication_attempt(
    result: Result<Option<AnalysisPublicationOutputs>, OrchestratorError>,
) -> Result<AnalysisPublicationAttempt, OrchestratorError> {
    match result {
        Ok(Some(outputs)) => Ok(AnalysisPublicationAttempt::Published(outputs)),
        Ok(None) => Ok(AnalysisPublicationAttempt::NotRequested),
        Err(error) if error.is_cancelled() => Err(error),
        Err(OrchestratorError::Internal { problem }) => {
            Ok(AnalysisPublicationAttempt::Unavailable { reason: problem })
        }
        Err(error) => Ok(AnalysisPublicationAttempt::Unavailable {
            reason: error.to_string(),
        }),
    }
}

fn try_stage_analysis(
    prep: &GenerationPreparation,
    head: &str,
    context: Option<&AnalysisPublicationContext>,
    runner_temp: &Path,
) -> Result<Option<AnalysisPublicationOutputs>, OrchestratorError> {
    let Some(context) = context.filter(|context| {
        runtime_trust::publication_context_matches(context) && context.qualifies(prep, head)
    }) else {
        return Ok(None);
    };
    if prep.discovery.raw_inventories.is_empty() || prep.discovery.rust_inventory.is_some() {
        return Ok(None);
    }
    let catalog = ToolCatalog::pinned();
    let helper_sha256 = crate::cover_identity::generator::current_exe_sha256()
        .ok_or_else(|| internal("analysis_helper_unverifiable"))?;
    let (index, skipped) =
        crate::discover_index::build_file_index(&prep.root, &prep.config.discovery.exclude)?;
    if skipped {
        return Err(internal("analysis_inventory_non_utf8"));
    }
    let identity = AnalysisIdentity {
        helper_sha256: helper_sha256.clone(),
        cargo_identity: observed_cargo(&catalog, &prep.root)?,
        cargo_pin: catalog.rustup_toolchain().to_owned(),
        resolution_inputs_digest: resolution_inputs_digest(
            &prep.root,
            index.files(),
            &prep.discovery.raw_inventories,
        )
        .map_err(|problem| internal(&problem))?,
        source: AnalysisSource {
            repository: context.repository.clone(),
            head_sha: head.to_owned(),
            workflow_sha: context.workflow_sha.clone(),
            run_id: context.run_id,
            run_attempt: context.run_attempt,
            branch: context.branch.clone(),
        },
    };
    let text = build_payload(&prep.root, identity, &prep.discovery.raw_inventories)
        .map_err(|problem| internal(&problem))?;
    let artifact_name = crate::analysis_inventory::authority::analysis_artifact_name(
        head,
        &helper_sha256,
        &catalog.rustup_toolchain(),
    )
    .map_err(|problem| internal(&problem))?;
    let dir = runner_temp
        .join("velnor-analysis")
        .join(format!("r{}-a{}", context.run_id, context.run_attempt));
    crate::exclusive_write::create_dir_no_symlink(runner_temp, &dir)?;
    let artifact_path = dir.join("analysis.json");
    crate::exclusive_write::write_exclusive(&artifact_path, text.as_bytes(), "analysis_artifact")?;
    Ok(Some(AnalysisPublicationOutputs {
        artifact_name,
        artifact_path,
    }))
}

/// Read the actual selected Cargo, using the same pinned isolated execution policy.
fn observed_cargo(catalog: &ToolCatalog, root: &Path) -> Result<String, OrchestratorError> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        vec![OsString::from("--version")],
    )
    .map_err(|error| internal(&error.to_string()))?;
    let output = request
        .command(catalog)
        .map_err(|error| internal(&error.to_string()))?
        .with_cwd(root.to_path_buf())
        .run()
        .map_err(|error| internal(&error.to_string()))?;
    output
        .require_success("cargo")
        .map_err(|error| internal(&error.to_string()))?;
    let text = output
        .stdout_text("cargo")
        .map_err(|error| internal(&error.to_string()))?;
    let identity = text.trim().to_owned();
    if !valid_cargo_identity(&identity) {
        return Err(internal("analysis_cargo_identity_mismatch"));
    }
    Ok(identity)
}

fn valid_cargo_identity(identity: &str) -> bool {
    let parts: Vec<_> = identity.split(' ').collect();
    let ["cargo", version, commit, date] = parts.as_slice() else {
        return false;
    };
    let version: Vec<_> = version.split('.').collect();
    let commit = commit.strip_prefix('(').unwrap_or_default();
    let date = date.strip_suffix(')').unwrap_or_default();
    version.len() == 3
        && version
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && (7..=40).contains(&commit.len())
        && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        && date.len() == 10
        && date.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_misses_are_explicit_unavailability_and_cancellation_stays_failure() {
        for reason in [
            "analysis_membership_unsupported_glob",
            "analysis_inventory_target_path_limit",
            "symlink_refused",
        ] {
            assert_eq!(
                publication_attempt(Err(internal(reason))).expect("publication miss"),
                AnalysisPublicationAttempt::Unavailable {
                    reason: reason.to_owned()
                },
            );
        }
        assert_eq!(
            publication_attempt(Ok(None)).expect("ineligible"),
            AnalysisPublicationAttempt::NotRequested
        );
        assert!(
            publication_attempt(Err(OrchestratorError::cancelled("analysis", "signal"))).is_err()
        );
    }

    #[test]
    fn observed_identity_requires_cargo_release_shape() {
        assert!(valid_cargo_identity("cargo 1.98.0 (01234567 2026-08-01)"));
        assert!(valid_cargo_identity("cargo 1.98.1 (01234567 2026-08-01)"));
        assert!(!valid_cargo_identity(
            "cargo 1.98.0 (01234567 2026-08-01)\nforged"
        ));
        assert!(!valid_cargo_identity(
            "cargo 1.98.0-forged (01234567 2026-08-01)"
        ));
    }

    #[test]
    fn publication_requires_protected_same_source_default_push() {
        let head = "a".repeat(40);
        let context = AnalysisPublicationContext {
            repository: "owner/repo".to_owned(),
            head: head.clone(),
            workflow_sha: head.clone(),
            workflow_ref: format!(
                "owner/repo/{}@refs/heads/main",
                velnor_actions_workflow_renderer::WORKFLOW_PATH
            ),
            branch: "main".to_owned(),
            default_branch: "main".to_owned(),
            run_id: 1,
            run_attempt: 1,
            protected: true,
            event: "push".to_owned(),
        };
        assert!(context.matches_source(&head, "main"));
        for field in [
            "protected",
            "event",
            "head",
            "workflow_sha",
            "workflow_ref",
            "branch",
            "default_branch",
            "run_id",
            "run_attempt",
            "repository",
        ] {
            let mut invalid = context.clone();
            match field {
                "protected" => invalid.protected = false,
                "event" => invalid.event = "pull_request".to_owned(),
                "head" => invalid.head = "b".repeat(40),
                "workflow_sha" => invalid.workflow_sha = "b".repeat(40),
                "workflow_ref" => invalid.workflow_ref.push_str("-forged"),
                "branch" => invalid.branch = "topic".to_owned(),
                "default_branch" => invalid.default_branch = "topic".to_owned(),
                "run_id" => invalid.run_id = 0,
                "run_attempt" => invalid.run_attempt = 0,
                "repository" => invalid.repository = "fork/repo".to_owned(),
                _ => unreachable!(),
            }
            assert!(!invalid.matches_source(&head, "main"), "{field}");
        }
    }

    #[test]
    fn workflow_ref_normalizes_repository_case_only() {
        let head = "a".repeat(40);
        let mut context = AnalysisPublicationContext {
            repository: "chainargos/caserustworkspace".to_owned(),
            head: head.clone(),
            workflow_sha: head.clone(),
            workflow_ref: format!(
                "ChainArgos/CaseRustWorkspace/{}@refs/heads/Main",
                velnor_actions_workflow_renderer::WORKFLOW_PATH
            ),
            branch: "Main".to_owned(),
            default_branch: "Main".to_owned(),
            run_id: 1,
            run_attempt: 1,
            protected: true,
            event: "push".to_owned(),
        };
        assert!(context.matches_source(&head, "Main"));
        assert!(!context.matches_source(&head, "main"));
        context.workflow_ref = context.workflow_ref.replace("/Main", "/main");
        assert!(!context.matches_source(&head, "Main"));
        context.workflow_ref =
            "ChainArgos/CaseRustWorkspace/.github/workflows/CI.yml@refs/heads/Main".to_owned();
        assert!(!context.matches_source(&head, "Main"));
        context.workflow_ref = format!(
            "Fork/CaseRustWorkspace/{}@refs/heads/Main",
            velnor_actions_workflow_renderer::WORKFLOW_PATH
        );
        assert!(!context.matches_source(&head, "Main"));
    }

    #[test]
    fn publication_context_rejects_unknown_authority_fields() {
        let value = serde_json::json!({
            "repository":"owner/repo", "head":"a".repeat(40),
            "workflow_sha":"a".repeat(40), "workflow_ref":"owner/repo/.github/workflows/ci.yml@refs/heads/main",
            "branch":"main", "default_branch":"main", "run_id":1, "run_attempt":1,
            "protected":true, "event":"push", "trusted":true,
        });
        assert!(serde_json::from_value::<AnalysisPublicationContext>(value).is_err());
    }
}
