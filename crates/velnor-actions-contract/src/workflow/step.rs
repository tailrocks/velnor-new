//! Typed workflow step identity and semantic role.

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Stable GitHub Actions id for a step whose outputs have consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepId {
    /// Planner output consumed by downstream jobs.
    Plan,
    /// Baseline publisher output consumed by the release uploader.
    PublishBaseline,
    /// MBX cache action output consumed by the MBX bundle steps.
    Mbx,
    /// MBX bundle-key output consumed by its restore step.
    MbxBundleKey,
    /// MBX bundle restore output consumed by its import step.
    MbxBundle,
    /// MBX export output consumed by its save gate.
    MbxExport,
    /// OpenTofu provider-cache restore outputs consumed by admission.
    TofuProviders,
}

impl StepId {
    /// Fixed YAML spelling for this step output owner.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::PublishBaseline => "publish-baseline",
            Self::Mbx => "mbx",
            Self::MbxBundleKey => "mbx-bundle-key",
            Self::MbxBundle => "mbx-bundle",
            Self::MbxExport => "mbx-export",
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
    /// Install the exact catalog tools used by later steps.
    PreparePinnedTools,
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
    /// Upload of one matrix task report.
    MatrixReportUpload,
    /// Download of a candidate attestation artifact.
    AttestationDownload,
    /// Fetch of exact matrix report artifacts.
    FetchReports,
    /// Upload of the final merged report.
    PublishFinal,
    /// OpenTofu/Rust plan formatting step.
    PlanFormat,
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
    /// One elected OpenTofu provider-cache writer.
    TofuProvidersSave,
    /// OpenTofu provider-cache restore whose output is admitted before use.
    TofuProvidersRestore,
    /// OpenTofu provider-cache admission before any provider consumer.
    TofuProvidersAdmission,
    /// Pre-seed build of the candidate helper.
    PreseedBuild,
    /// Pre-seed manifest creation.
    PreseedManifest,
    /// Pre-seed candidate artifact upload.
    PreseedUpload,
    /// Pre-seed candidate artifact download.
    PreseedDownload,
    /// Pre-seed manifest verification.
    PreseedVerifyManifest,
    /// MBX provider-cache import from its bundle.
    MbxBundleImport,
    /// MBX bundle export from the live object store.
    MbxBundleExport,
    /// MBX single-bundle cache save.
    MbxBundleSave,
    /// MBX action cache restore.
    MbxCache,
    /// MBX cache-primary-key prefix producer.
    MbxBundleKey,
    /// MBX bundle cache restore.
    MbxBundleRestore,
}

impl StepRole {
    /// Check role-specific payload kind and identity.
    fn validate(self, kind: &StepKind, job: &str) -> Result<(), ContractError> {
        let valid = match self {
            Self::Checkout => matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")),
            Self::PreparePinnedTools => matches!(kind, StepKind::Shell { .. }),
            Self::AcquireVelnor
            | Self::PreseedStage
            | Self::CheckGenerated
            | Self::PlanFormat
            | Self::MbxBundleExport => matches!(kind, StepKind::Shell { .. }),
            Self::TofuProvidersAdmission => matches!(kind, StepKind::Shell { .. }),
            Self::PlanProducer => matches!(kind, StepKind::Internal { operation } if operation == "plan-v1"),
            Self::BaselinePublisher => matches!(kind, StepKind::Internal { operation } if operation == "publish-baseline-v1"),
            Self::FetchReports => matches!(kind, StepKind::Internal { operation } if operation == "fetch-reports-v1"),
            Self::DownloadPlan | Self::AttestationDownload | Self::PreseedDownload => {
                matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/download-artifact@"))
            }
            Self::PublishPlan
            | Self::MatrixReportUpload
            | Self::PublishFinal
            | Self::PreseedUpload
                => matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/upload-artifact@")),
            Self::ToolsCacheSave | Self::TofuProvidersSave | Self::MbxBundleSave => {
                matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/cache/save@"))
            }
            Self::CargoSourcesRestore => {
                matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/cache/restore@"))
            }
            Self::CargoSourcesSave => {
                matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/cache/save@"))
            }
            Self::CargoSourcesFetch => matches!(kind, StepKind::Shell { .. }),
            Self::CargoRegistryRestore => {
                matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("Swatinem/rust-cache@"))
            }
            Self::MbxBundleRestore => {
                matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/cache/restore@"))
            }
            Self::TofuProvidersRestore => matches!(kind, StepKind::Action { uses, with, .. } if uses.starts_with("actions/cache/restore@") && with.get("restore-keys").is_some_and(String::is_empty)),
            Self::PreseedBuild | Self::PreseedManifest | Self::PreseedVerifyManifest
            | Self::MbxBundleImport | Self::MbxBundleKey => matches!(kind, StepKind::Shell { .. }),
            Self::MbxCache => matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("jdx/mr-boxington-action@")),
        };
        if valid {
            Ok(())
        } else {
            Err(ContractError::identity(
                "step.role",
                format!("role_kind_mismatch:{job}:{self:?}"),
            ))
        }
    }

    /// Output identity required for a role that publishes step outputs.
    const fn required_id(self) -> Option<StepId> {
        match self {
            Self::PlanProducer => Some(StepId::Plan),
            Self::BaselinePublisher => Some(StepId::PublishBaseline),
            Self::MbxCache => Some(StepId::Mbx),
            Self::MbxBundleKey => Some(StepId::MbxBundleKey),
            Self::MbxBundleRestore => Some(StepId::MbxBundle),
            Self::MbxBundleExport => Some(StepId::MbxExport),
            Self::TofuProvidersRestore => Some(StepId::TofuProviders),
            _ => None,
        }
    }

    /// Role required for a typed step output identity.
    const fn for_id(id: StepId) -> Self {
        match id {
            StepId::Plan => Self::PlanProducer,
            StepId::PublishBaseline => Self::BaselinePublisher,
            StepId::Mbx => Self::MbxCache,
            StepId::MbxBundleKey => Self::MbxBundleKey,
            StepId::MbxBundle => Self::MbxBundleRestore,
            StepId::MbxExport => Self::MbxBundleExport,
            StepId::TofuProviders => Self::TofuProvidersRestore,
        }
    }
}

/// One workflow step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Step name shown in workflow logs; presentation only.
    pub name: String,
    /// Stable GitHub Actions id for output-producing steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<StepId>,
    /// Typed semantic role; never inferred from the display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<StepRole>,
    /// Run condition (`if`), serialized by the workflow renderer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Step payload.
    #[serde(flatten)]
    pub kind: StepKind,
}

/// Step payload variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    /// Pinned GitHub Action step.
    Action {
        /// Full-SHA `uses` reference.
        uses: String,
        /// Action inputs.
        #[serde(default)]
        with: std::collections::BTreeMap<String, String>,
        /// Step environment (applies to main and post phases alike).
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },
    /// Fixed shell argv step.
    Shell {
        /// Fixed argument vector.
        run: Vec<String>,
        /// Fixed environment.
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },
    /// Fixed internal planner/aggregation step.
    Internal {
        /// Internal operation name.
        operation: String,
    },
}

impl Step {
    /// Validate one step payload and its typed authority.
    pub(crate) fn validate(&self, job: &str) -> Result<(), ContractError> {
        if self.name.trim().is_empty() {
            return Err(ContractError::identity(
                "step.name",
                format!("empty_name:{job}"),
            ));
        }
        if let Some(condition) = &self.condition
            && (condition.trim().is_empty() || condition.bytes().any(|b| b == b'\n' || b == b'\r'))
        {
            return Err(ContractError::identity(
                "step.condition",
                format!("bad_condition:{job}"),
            ));
        }
        if let Some(role) = self.role {
            role.validate(&self.kind, job)?;
            if let Some(expected) = role.required_id()
                && self.id != Some(expected)
            {
                return Err(ContractError::identity(
                    "step.id",
                    format!("role_id_mismatch:{job}:{role:?}"),
                ));
            }
        }
        if let Some(id) = self.id
            && self.role != Some(StepRole::for_id(id))
        {
            return Err(ContractError::identity(
                "step.role",
                format!("id_role_mismatch:{job}:{}", id.as_str()),
            ));
        }
        validate_id_kind(self.id, &self.kind, job)?;
        validate_kind(&self.kind, job)
    }
}

/// Require an output id to belong to its typed payload kind.
fn validate_id_kind(id: Option<StepId>, kind: &StepKind, job: &str) -> Result<(), ContractError> {
    let Some(id) = id else { return Ok(()) };
    let valid = match id {
        StepId::Plan => matches!(kind, StepKind::Internal { operation } if operation == "plan-v1"),
        StepId::PublishBaseline => matches!(kind, StepKind::Internal { operation } if operation == "publish-baseline-v1"),
        StepId::Mbx => matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("jdx/mr-boxington-action@")),
        StepId::MbxBundleKey | StepId::MbxExport => matches!(kind, StepKind::Shell { .. }),
        StepId::MbxBundle => matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/cache/restore@")),
        StepId::TofuProviders => matches!(kind, StepKind::Action { uses, with, .. } if uses.starts_with("actions/cache/restore@") && with.get("restore-keys").is_some_and(String::is_empty)),
    };
    if valid {
        Ok(())
    } else {
        Err(ContractError::identity(
            "step.id",
            format!("id_kind_mismatch:{job}:{id:?}"),
        ))
    }
}

/// Validate the payload-specific required fields.
fn validate_kind(kind: &StepKind, job: &str) -> Result<(), ContractError> {
    match kind {
        StepKind::Action { uses, .. } => {
            if uses.trim().is_empty() {
                return Err(ContractError::identity(
                    "step.uses",
                    format!("empty_uses:{job}"),
                ));
            }
        }
        StepKind::Shell { run, .. } => {
            if run.is_empty() || run.iter().any(|arg| arg.trim().is_empty()) {
                return Err(ContractError::identity(
                    "step.run",
                    format!("bad_argv:{job}"),
                ));
            }
        }
        StepKind::Internal { operation } => {
            if operation.trim().is_empty() {
                return Err(ContractError::identity(
                    "step.operation",
                    format!("empty_operation:{job}"),
                ));
            }
        }
    }
    Ok(())
}
