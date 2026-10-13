//! Typed Rust task-identity extension (cache §1).
//!
//! Unknown schemas disable reuse; the orchestrator wraps the extension in
//! the stack-neutral envelope before selection.

use serde::Serialize;
use velnor_actions_contract::ContractError;

use crate::profile::TestRunner;
use crate::tasks::TaskKind;

/// Typed Rust task-identity extension (cache §1); unknown schemas disable reuse.
#[derive(Debug, Clone, Serialize)]
pub struct RustTaskIdentityExtension {
    /// Cargo package ID.
    pub package_id: String,
    /// Workspace identity digest.
    pub workspace_id: String,
    /// Execution profile (configuration) name.
    pub profile: String,
    /// Normalized manifest path.
    pub manifest: String,
    /// Workspace/local-package graph digest.
    pub graph_digest: String,
    /// Target kinds and names, sorted.
    pub targets: Vec<String>,
    /// Enabled features, sorted.
    pub features: Vec<String>,
    /// Rust target and profile.
    pub target: String,
    /// Compile driver plus test runner (`driver+runner`).
    pub driver: String,
    /// Cargo config and build-script input digests.
    pub config_digest: String,
    /// `.config/nextest.toml` digest for Nextest profiles.
    pub nextest_digest: Option<String>,
    /// Rust task kind plus test/archive identity.
    pub kind: String,
    /// Build script reads undeclared inputs; disables reuse and coverage.
    pub undeclared_reads: bool,
    /// `Cargo.lock` digest, when the lockfile is available.
    pub lock_digest: Option<String>,
    /// Archive identity (producing build task id) for Nextest archives only.
    pub archive: Option<String>,
    /// Declared `rerun-if-changed` build inputs, sorted.
    pub rerun_inputs: Vec<String>,
    /// Declared non-Rust task inputs, sorted.
    pub declared_inputs: Vec<String>,
}

/// Inputs for deriving one task-identity extension before selection.
#[derive(Debug, Clone)]
pub struct ExtensionInputs<'a> {
    /// Cargo package ID.
    pub package_id: &'a str,
    /// Workspace identity digest.
    pub workspace_id: &'a str,
    /// Execution profile (configuration) name.
    pub profile: &'a str,
    /// Normalized manifest path.
    pub manifest: &'a str,
    /// Workspace/local-package graph digest.
    pub graph_digest: &'a str,
    /// Target kinds and names.
    pub targets: &'a [String],
    /// Enabled features.
    pub features: &'a [String],
    /// Rust target and profile.
    pub target: &'a str,
    /// Compile driver.
    pub driver: &'a str,
    /// Test runner.
    pub runner: &'a str,
    /// Cargo config and build-script input digests.
    pub config_digest: &'a str,
    /// `Cargo.lock` digest, when the lockfile is available.
    pub lock_digest: Option<&'a str>,
    /// `.config/nextest.toml` digest for Nextest profiles.
    pub nextest_digest: Option<&'a str>,
    /// Rust task kind.
    pub kind: TaskKind,
    /// Build task id producing the archive (Nextest `Build` only).
    pub archive_source: Option<&'a str>,
    /// Declared `rerun-if-changed` inputs (`None` means unknown).
    pub rerun_inputs: Option<&'a [String]>,
    /// Whether the package carries a build script.
    pub has_build_script: bool,
    /// Declared non-Rust task inputs.
    pub declared_inputs: &'a [String],
}

impl RustTaskIdentityExtension {
    /// Wrap the extension in the stack-neutral envelope.
    #[must_use]
    pub fn to_stack_extension(&self) -> velnor_actions_contract::StackExtension {
        velnor_actions_contract::StackExtension {
            schema: "rust-task-identity-v1".to_owned(),
            data: serde_json::to_value(self).unwrap_or(serde_json::Value::Null),
        }
    }

    /// Reject reuse when build inputs are undeclared or dynamic.
    /// # Errors
    pub fn reuse_eligible(&self) -> Result<(), ContractError> {
        if self.undeclared_reads {
            return Err(ContractError::identity(
                "stack_extension",
                "undeclared_inputs",
            ));
        }
        Ok(())
    }

    /// Derive the extension for one task group before selection and reuse.
    ///
    /// Archives attach only to Nextest `Build` groups (cargo-test never
    /// archives; the archive carries its producing build task id so trust
    /// and retention follow the source). A build script with unknown
    /// `rerun-if-changed` inputs conservatively disables reuse.
    #[must_use]
    pub fn for_task(inputs: &ExtensionInputs<'_>) -> Self {
        let nextest = inputs.runner == TestRunner::CargoNextest.as_str();
        let archive = if inputs.kind == TaskKind::Build && nextest {
            inputs.archive_source.map(str::to_owned)
        } else {
            None
        };
        Self {
            package_id: inputs.package_id.to_owned(),
            workspace_id: inputs.workspace_id.to_owned(),
            profile: inputs.profile.to_owned(),
            manifest: inputs.manifest.to_owned(),
            graph_digest: inputs.graph_digest.to_owned(),
            targets: sorted_unique(inputs.targets),
            features: sorted_unique(inputs.features),
            target: inputs.target.to_owned(),
            driver: format!("{}+{}", inputs.driver, inputs.runner),
            config_digest: inputs.config_digest.to_owned(),
            nextest_digest: inputs.nextest_digest.map(str::to_owned),
            kind: inputs.kind.as_str().to_owned(),
            undeclared_reads: inputs.has_build_script && inputs.rerun_inputs.is_none(),
            lock_digest: inputs.lock_digest.map(str::to_owned),
            archive,
            rerun_inputs: inputs.rerun_inputs.map_or_else(Vec::new, sorted_unique),
            declared_inputs: sorted_unique(inputs.declared_inputs),
        }
    }
}

/// Sorted deduped copy for identity stability.
fn sorted_unique(values: &[String]) -> Vec<String> {
    let mut out = values.to_vec();
    out.sort();
    out.dedup();
    out
}
