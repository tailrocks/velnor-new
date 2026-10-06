//! Source-only compiler qualification recipe, separate from installed SDK authority.

use std::collections::BTreeMap;

use crate::{
    MiseError,
    catalog::rust_compiler_authority::RootRustCandidateSource,
    root_rust_candidate_root::{RootRustCandidateLeaf, RootRustCandidateRoot},
};

#[path = "catalog_root_rust_candidate_body.rs"]
mod body;

#[cfg(test)]
#[path = "catalog_root_rust_candidate_tests.rs"]
mod tests;

/// Compiled stage; callers cannot replace its source, arguments, or role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRustCandidateStage {
    name: &'static str,
    source: String,
}

impl RootRustCandidateStage {
    /// Fixed phase name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }
    /// Exact executable source bytes, requiring a qualified runtime before execution.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// No runtime selectors or caller configuration are accepted.
    #[must_use]
    pub const fn arguments(&self) -> &[String] {
        &[]
    }
    /// Fixed native executable closure; the Foundation owner must qualify it before execution.
    #[must_use]
    pub const fn executable_roster(&self) -> &'static [&'static str] {
        crate::catalog::rust_bootstrap::RustupBootstrap::root_candidate_executable_roster()
    }
    /// Closed system search directories used before the manager-only install PATH.
    #[must_use]
    pub const fn executable_search_paths(&self) -> &'static [&'static str] {
        &["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
    }
    /// Source transport integrity, never an installed compiler grant.
    #[must_use]
    pub fn source_sha256(&self) -> String {
        velnor_actions_contract::compiled_source_sha256(self.source.as_bytes())
    }
}

/// Opaque source-only candidate. No conversion to an installed compiler or SDK exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRustCompilerCandidate {
    root: RootRustCandidateRoot,
    inputs: RootRustCandidateSource,
    stages: [RootRustCandidateStage; 3],
    environment: BTreeMap<String, String>,
    version: String,
}

impl RootRustCompilerCandidate {
    /// Emit the first qualification recipe without requiring its future installed receipt.
    /// # Errors
    /// Rejects inconsistent compiled source inputs or invalid generator source markers.
    pub fn root_linux(version: &str) -> Result<Self, MiseError> {
        let root = RootRustCandidateRoot::root_linux();
        let inputs = RootRustCandidateSource::require_root_linux()?;
        let stages = [
            RootRustCandidateStage {
                name: "clear",
                source: body::clear(root, version)?,
            },
            RootRustCandidateStage {
                name: "acquire",
                source: body::acquire(root, &inputs, version)?,
            },
            RootRustCandidateStage {
                name: "install",
                source: body::install(root, &inputs, version)?,
            },
        ];
        Ok(Self {
            root,
            inputs,
            stages,
            environment: environment(root),
            version: version.to_owned(),
        })
    }

    /// Ordered source phases; execution requires the runtime owner's private Foundation issuer.
    #[must_use]
    pub const fn stages(&self) -> &[RootRustCandidateStage; 3] {
        &self.stages
    }
    /// Fixed homes and policies; no cache, Mise, workflow outputs, or credentials.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
    /// Read-only candidate record. Observations cannot mint an SDK or installed authority.
    #[must_use]
    pub fn projection(&self) -> serde_json::Value {
        let manager = crate::catalog::rust_bootstrap::RustupBootstrap::for_host(self.root.host());
        let stages: Vec<_> = self.stages.iter().map(|stage| serde_json::json!({
            "name": stage.name(), "source": stage.source(), "source_sha256": stage.source_sha256(), "arguments": stage.arguments(),
            "executable_roster": stage.executable_roster(),
            "executable_search_paths": stage.executable_search_paths(),
        })).collect();
        serde_json::json!({
            "schema": 1, "purpose": "root-linux-compiler-candidate", "role": "root-linux",
            "host": self.root.host().target_triple(), "rust_version": self.root.compiler_version(),
            "toolchain": format!("{}-{}", self.root.compiler_version(), self.root.host().target_triple()),
            "namespace": self.root.relative_to_runner_temp(),
            "leaves": self.root.leaves().iter().map(|leaf| leaf.relative()).collect::<Vec<_>>(),
            "stages": stages, "environment": self.environment,
            "manager_sha256": manager.sha256(), "manager_version": manager.version(),
            "native_source_authority": self.inputs.projection(),
            "installer_source_identity": velnor_actions_contract::compiled_source_sha256(self.stages.iter().map(RootRustCandidateStage::source).collect::<Vec<_>>().join("\0").as_bytes()),
        })
    }
    /// Reconstruct the complete fixed recipe before source transport.
    /// # Errors
    /// Rejects stale source, authority, or environment bindings.
    pub fn verify_fresh(&self) -> Result<(), MiseError> {
        if &Self::root_linux(&self.version)? != self {
            return Err(MiseError::Contract {
                problem: "root_rust_candidate_binding_changed".to_owned(),
            });
        }
        Ok(())
    }
}

fn environment(root: RootRustCandidateRoot) -> BTreeMap<String, String> {
    let (key, value) = root.namespace_environment();
    [
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        (key.to_owned(), value.to_owned()),
        (
            "CARGO_HOME".to_owned(),
            root.leaf_expression(RootRustCandidateLeaf::CargoHome),
        ),
        (
            "RUSTUP_HOME".to_owned(),
            root.leaf_expression(RootRustCandidateLeaf::RustupHome),
        ),
        (
            "RUSTUP_TOOLCHAIN".to_owned(),
            format!(
                "{}-{}",
                root.compiler_version(),
                root.host().target_triple()
            ),
        ),
        ("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned()),
    ]
    .into_iter()
    .collect()
}
