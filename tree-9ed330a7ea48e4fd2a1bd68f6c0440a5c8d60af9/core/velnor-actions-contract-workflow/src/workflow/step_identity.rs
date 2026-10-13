//! Typed workflow step identity and semantic role.

use super::step::{Step, StepKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use velnor_actions_contract::errors::ContractError;

/// Fixed local action path used for exact provider-cache admission.
pub const TOFU_PROVIDER_ADMISSION_USES: &str = "./.github/actions/tofu-provider-admission";
/// Fixed local action path for the exact-key host tool seed.
pub const TOOL_SEED_USES: &str = "./.github/actions/velnor-tool-seed";
/// Expression path for the job-private `OpenTofu` plugin cache.
pub const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-cache";
/// Exact-key layer identity shared by the restore and save protocol.
pub const TOFU_PROVIDERS_KEY_PREFIX: &str = "velnor-v1-tofu-providers";
/// Composite output expression for the exact configured provider key.
pub const TOFU_PROVIDERS_KEY_OUTPUT_EXPR: &str = "${{ steps.tofu-providers.outputs.cache-key }}";
/// Composite output expression for the owned provider-cache path.
pub const TOFU_PROVIDERS_PATH_OUTPUT_EXPR: &str = "${{ steps.tofu-providers.outputs.cache-path }}";
/// Runtime-gated cache input for a Mise setup whose hosted image was verified.
pub const MISE_CACHE_ENABLED_EXPR: &str = "${{env.VELNOR_MISE_CACHE_ENABLED}}";

/// Stable GitHub Actions id for a step whose outputs have consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepId {
    /// Planner output consumed by downstream jobs.
    Plan,
    /// Baseline publisher output consumed by the release uploader.
    PublishBaseline,
    /// `OpenTofu` provider-cache composite outputs consumed by the save step.
    TofuProviders,
}

impl StepId {
    /// Fixed YAML spelling for this step output owner.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::PublishBaseline => "publish-baseline",
            Self::TofuProviders => "tofu-providers",
        }
    }
}

/// Semantic step role used for workflow composition and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepRole {
    /// Checkout whose credentials are disabled.
    Checkout,
    /// Copy from the trusted, exact-key host tool seed.
    ToolSeed,
    /// Install the exact catalog tools used by later steps.
    PreparePinnedTools,
    /// Install the pinned Rust components used by task work.
    PrepareRustComponents,
    /// Pinned Mise action that owns its built-in tools cache.
    MiseSetup,
    /// Authenticated cargo-deny analyzer bootstrap.
    CargoDeny,
    /// Authenticated cargo-machete analyzer bootstrap.
    CargoMachete,
    /// Authenticated actionlint analyzer bootstrap.
    Actionlint,
    /// Authenticated zizmor analyzer bootstrap.
    Zizmor,
    /// Planner producing the plan outputs.
    PlanProducer,
    /// Baseline publisher producing the artifact name output.
    BaselinePublisher,
    /// Digest-verified Velnor executable acquisition.
    AcquireVelnor,
    /// Pre-seed copy of the helper into runner-temp staging.
    PreseedStage,
    /// Download of the plan artifact for final merging.
    DownloadPlan,
    /// Generated-tree freshness check.
    CheckGenerated,
    /// Upload of the planner report artifact.
    PublishPlan,
    /// Upload of a crate or matrix task report artifact.
    MatrixReportUpload,
    /// Verify task matrix context and stage declared task outputs.
    ArtifactBuildExport,
    /// Upload one verified, run-scoped artifact build result.
    ArtifactBuildUpload,
    /// Download of a candidate attestation artifact.
    AttestationDownload,
    /// Fetch of exact matrix report artifacts.
    FetchReports,
    /// Upload of the final merged report.
    PublishFinal,
    /// `OpenTofu`/Rust plan formatting step.
    PlanFormat,
    /// Per-crate minimum-supported-Rust-version qualification.
    MsrvQualification,
    /// One elected tools-cache writer.
    ToolsCacheSave,
    /// Restore of the shared Cargo registry and git sources snapshot.
    CargoSourcesRestore,
    /// Single elected writer for the shared Cargo sources snapshot.
    CargoSourcesSave,
    /// Fetch probe used to populate the Cargo source snapshot on a miss.
    CargoSourcesFetch,
    /// Cargo-only registry cache restore.
    CargoRegistryRestore,
    /// One elected `OpenTofu` provider-cache writer.
    TofuProvidersSave,
    /// `OpenTofu` provider-cache restore/admission composite before provider use.
    TofuProvidersRestore,
    /// `OpenTofu` init/validate shell that consumes the admitted plugin cache.
    TofuProviderUse,
    /// Pre-seed build of the candidate helper.
    PreseedBuild,
    /// Pre-seed manifest creation.
    PreseedManifest,
    /// Verification of the freshly built pre-seed candidate.
    PreseedVerifyBuild,
    /// Pre-seed candidate artifact upload.
    PreseedUpload,
    /// Pre-seed candidate artifact download.
    PreseedDownload,
    /// Pre-seed manifest verification.
    PreseedVerifyManifest,
    /// Native MBX action owns hosted and local object-cache lifecycle.
    MbxCache,
    /// Exact MBX binary and object-store verification after the native action.
    MbxVersionCheck,
    /// Exact MBX and Rust toolchain preflight.
    MbxPreflight,
}

impl StepRole {
    /// Check role-specific payload kind and identity.
    pub(crate) fn validate(self, kind: &StepKind, job: &str) -> Result<(), ContractError> {
        if self.matches_payload(kind) {
            Ok(())
        } else {
            Err(ContractError::identity(
                "step.role",
                format!("role_kind_mismatch:{job}:{self:?}"),
            ))
        }
    }

    /// Match one role against its fixed kind and payload family.
    fn matches_payload(self, kind: &StepKind) -> bool {
        match self {
            Self::Checkout => valid_checkout_payload(kind),
            Self::ToolSeed => valid_tool_seed_payload(kind),
            Self::MiseSetup => valid_mise_setup(kind),
            Self::TofuProviderUse => super::step_protocol::valid_provider_use(kind),
            Self::PlanProducer => internal_operation(kind, "plan-v1"),
            Self::BaselinePublisher => internal_operation(kind, "publish-baseline-v1"),
            Self::ArtifactBuildExport => internal_operation(kind, "export-artifact-v1"),
            Self::FetchReports => internal_operation(kind, "fetch-reports-v1"),
            Self::DownloadPlan | Self::AttestationDownload | Self::PreseedDownload => {
                action_has_prefix_for_kind(kind, "actions/download-artifact@")
            }
            Self::PublishPlan
            | Self::MatrixReportUpload
            | Self::ArtifactBuildUpload
            | Self::PublishFinal
            | Self::PreseedUpload => action_has_prefix_for_kind(kind, "actions/upload-artifact@"),
            Self::ToolsCacheSave | Self::CargoSourcesSave => {
                action_has_prefix_for_kind(kind, "actions/cache/save@")
            }
            Self::CargoSourcesRestore => action_has_prefix_for_kind(kind, "actions/cache/restore@"),
            Self::TofuProvidersRestore => super::step_protocol::valid_provider_restore(kind),
            Self::TofuProvidersSave => super::step_protocol::valid_provider_save(kind),
            Self::CargoRegistryRestore => action_has_prefix_for_kind(kind, "Swatinem/rust-cache@"),
            Self::MbxCache => valid_mbx_cache(kind),
            Self::MbxVersionCheck => matches!(kind, StepKind::Shell { .. }),
            Self::PreparePinnedTools
            | Self::PrepareRustComponents
            | Self::CargoDeny
            | Self::CargoMachete
            | Self::Actionlint
            | Self::Zizmor
            | Self::AcquireVelnor
            | Self::PreseedStage
            | Self::CheckGenerated
            | Self::PlanFormat
            | Self::MsrvQualification
            | Self::CargoSourcesFetch
            | Self::PreseedBuild
            | Self::PreseedManifest
            | Self::PreseedVerifyBuild
            | Self::PreseedVerifyManifest
            | Self::MbxPreflight => matches!(kind, StepKind::Shell { .. }),
        }
    }

    /// Output identity required for a role that publishes step outputs.
    pub(crate) const fn required_id(self) -> Option<StepId> {
        match self {
            Self::PlanProducer => Some(StepId::Plan),
            Self::BaselinePublisher => Some(StepId::PublishBaseline),
            Self::TofuProvidersRestore => Some(StepId::TofuProviders),
            _ => None,
        }
    }

    /// Role required for a typed step output identity.
    pub(crate) const fn for_id(id: StepId) -> Self {
        match id {
            StepId::Plan => Self::PlanProducer,
            StepId::PublishBaseline => Self::BaselinePublisher,
            StepId::TofuProviders => Self::TofuProvidersRestore,
        }
    }
}

/// Check that a step is the configured checkout owner in this workflow scope.
///
/// Checkout is a semantic role, not a display-name convention. The caller
/// supplies the exact pinned action ref from its validated render context.
#[must_use]
pub fn is_configured_checkout(step: &Step, expected_uses: &str) -> bool {
    step.role == Some(StepRole::Checkout)
        && step.condition.is_none()
        && matches!(&step.kind, StepKind::Action { uses, .. } if uses == expected_uses)
        && valid_configured_checkout_payload(&step.kind)
}

/// Check that a step is the unconditional local tool-seed consumer.
#[must_use]
pub fn is_tool_seed_step(step: &Step) -> bool {
    step.role == Some(StepRole::ToolSeed)
        && step.condition.is_none()
        && valid_tool_seed_payload(&step.kind)
}

/// Require the standard credential-free checkout action payload.
fn valid_checkout_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if action_has_prefix(uses, "actions/checkout@")
            && with.get("persist-credentials").is_some_and(|value| value == "false"))
}

/// Require the simple generated checkout shape used before host-seed reads.
fn valid_configured_checkout_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { with, env, .. }
        if valid_checkout_payload(kind)
            && (with.len() == 1
                || (with.len() == 2
                    && with.get("fetch-depth").is_some_and(|value| value == "0")))
            && env.is_empty())
}

/// Require the fixed local seed action and its one explicit cache identity.
fn valid_tool_seed_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
        if uses == TOOL_SEED_USES
            && with.len() == 1
            && with.get("cache_key").is_some_and(|key| !key.is_empty())
            && env.is_empty())
}

/// True when an action payload uses a fixed name prefix.
fn action_has_prefix_for_kind(kind: &StepKind, prefix: &str) -> bool {
    matches!(kind, StepKind::Action { uses, .. } if action_has_prefix(uses, prefix))
}

/// True when a pinned action ref starts with its required owner/repository.
fn action_has_prefix(uses: &str, prefix: &str) -> bool {
    uses.starts_with(prefix)
}

/// True when an internal operation matches its role contract.
fn internal_operation(kind: &StepKind, expected: &str) -> bool {
    matches!(kind, StepKind::Internal { operation, .. } if operation == expected)
}

/// Validate the typed Mise setup payload shape shared by render paths.
fn valid_mise_setup(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if action_has_prefix(uses, "jdx/mise-action@")
            && with.get("version").is_some_and(|value| !value.is_empty())
            && with.get("sha256").is_some_and(|value| !value.is_empty())
            && with.get("install").is_some_and(|value| value == "false")
            && with.get("env").is_some_and(|value| value == "false")
            && with.get("cache").is_some_and(|value| {
                value == "false" || value == "true" || value == MISE_CACHE_ENABLED_EXPR
            })
            && with.get("cache_save").is_some_and(|value| value == "false" || value == "true"))
}

/// Validate the local provider admission call and exact same-restore outputs.
/// Validate the native MBX action that owns the object-cache lifecycle.
fn valid_mbx_cache(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if action_has_prefix(uses, "jdx/mr-boxington-action@")
            && with.get("github-cache-mode").is_some_and(|mode| mode == "objects")
            && with.get("version").is_some_and(|version| !version.is_empty())
            && with.get("toolchain").is_some_and(|toolchain| !toolchain.is_empty())
            && with.get("cache-generation").is_some_and(|generation| !generation.is_empty()))
}
/// Validate one serialized step scope, including role payloads and unique output IDs.
///
/// Workflow jobs and composite actions use separate output-ID scopes, so renderers
/// call this after internal expansion for each final serialized sequence.
/// # Errors
///
/// Returns a contract error for an invalid role/payload or duplicate step ID.
pub fn validate_step_sequence(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    validate_step_identities(steps, scope)?;
    super::step_protocol::validate_tofu_provider_sequence(steps, scope)
}

/// Validate IDs and role payloads in a partial serialized scope.
///
/// Use this for a composite body or an expanded workflow scope assembled
/// from only the steps serialized at that level. Call
/// [`validate_step_sequence`] on the full source sequence to enforce any
/// cross-step provider-cache protocol before factoring.
/// # Errors
///
/// Returns a contract error for an invalid role/payload or duplicate output ID.
pub fn validate_step_identity_scope(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    validate_step_identities(steps, scope)
}

/// Validate each step and require unique IDs within one serialized scope.
fn validate_step_identities(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    let mut ids = BTreeSet::new();
    for step in steps {
        step.validate(scope)?;
        if let Some(step_id) = step.id
            && !ids.insert(step_id)
        {
            return Err(ContractError::identity(
                "job.steps.id",
                format!("duplicate_step_id:{scope}:{}", step_id.as_str()),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod mise_cache_role_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn setup_step(cache: &str) -> Step {
        Step {
            name: "Setup Mise".to_owned(),
            id: None,
            role: Some(StepRole::MiseSetup),
            condition: None,
            kind: StepKind::Action {
                uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
                with: BTreeMap::from([
                    ("version".to_owned(), "2026.9.18".to_owned()),
                    ("sha256".to_owned(), "a".repeat(64)),
                    ("install".to_owned(), "false".to_owned()),
                    ("env".to_owned(), "false".to_owned()),
                    ("cache".to_owned(), cache.to_owned()),
                    ("cache_save".to_owned(), "false".to_owned()),
                ]),
                env: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn mise_setup_role_allows_only_the_exact_dynamic_cache_gate() {
        assert!(
            validate_step_identity_scope(&[setup_step(MISE_CACHE_ENABLED_EXPR)], "cache").is_ok()
        );
        assert!(validate_step_identity_scope(&[setup_step("false")], "cache").is_ok());
        assert!(
            validate_step_identity_scope(
                &[setup_step("${{github.event_name == 'push'}}")],
                "cache"
            )
            .is_err()
        );
    }
}
