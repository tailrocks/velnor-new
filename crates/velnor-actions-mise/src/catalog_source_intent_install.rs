//! Canonical cold installer image, with no workflow cache or runtime grant.

use std::collections::BTreeMap;

use crate::{
    MiseError,
    catalog::{
        mise_acquisition::{SourceIntentMiseAcquisition, source_intent_acquisition},
        qualification::qualified_toolset_digest,
        rust_compiler_authority::{GuardedRootRustInstallSource, RustCompilerArtifactAuthority},
    },
    source_intent_cold_root::{SourceIntentColdLeaf, SourceIntentColdRoot},
};

#[path = "catalog_source_intent_install_body.rs"]
mod body;

/// Private owner stage; callers cannot construct a source or argument binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIntentInstallStage {
    name: &'static str,
    source: String,
    arguments: Vec<String>,
}

impl SourceIntentInstallStage {
    /// Fixed stage name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }
    /// Exact source bytes.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Literal process arguments.
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }
    /// Complete source integrity binding.
    #[must_use]
    pub fn source_sha256(&self) -> String {
        velnor_actions_contract::compiled_source_sha256(self.source.as_bytes())
    }
}

/// Sealed generation data; the runtime owner alone may mint a cold SDK result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIntentColdInstaller {
    root: SourceIntentColdRoot,
    acquisition: SourceIntentMiseAcquisition,
    stages: [SourceIntentInstallStage; 3],
    environment: BTreeMap<String, String>,
    version: String,
    compiler: RustCompilerArtifactAuthority,
    guarded_install: GuardedRootRustInstallSource,
}

impl SourceIntentColdInstaller {
    /// Sole `RootLinux` purpose, always fresh and outside cache namespaces.
    /// # Errors
    /// Rejects absent owned `NoMiserc` publication or malformed source bindings.
    pub fn root_linux(version: &str) -> Result<Self, MiseError> {
        let compiler = RustCompilerArtifactAuthority::require_root_linux()?;
        let root = SourceIntentColdRoot::root_linux();
        let guarded_install = compiler.source_intent_install_source(root)?;
        let acquisition = source_intent_acquisition(root, version)?;
        let stages = [
            SourceIntentInstallStage {
                name: "clear",
                source: clear_source(root, version)?,
                arguments: vec![],
            },
            SourceIntentInstallStage {
                name: "acquire",
                source: acquisition.source().to_owned(),
                arguments: acquisition.args().to_vec(),
            },
            SourceIntentInstallStage {
                name: "install",
                source: body::source(
                    root,
                    version,
                    acquisition.distribution().binary_sha256(),
                    &guarded_install,
                )?,
                arguments: vec![],
            },
        ];
        let environment = canonical_environment(root, acquisition.environment());
        if guarded_install
            .environment()
            .iter()
            .any(|(key, value)| environment.get(key) != Some(value))
        {
            return Err(MiseError::Contract {
                problem: "source_intent_guarded_installer_environment_mismatch".to_owned(),
            });
        }
        Ok(Self {
            root,
            acquisition,
            stages,
            environment,
            version: version.to_owned(),
            compiler,
            guarded_install,
        })
    }

    /// Exact stages, in required fresh-genesis order.
    #[must_use]
    pub const fn stages(&self) -> &[SourceIntentInstallStage; 3] {
        &self.stages
    }
    /// Exact owner environment, without workflow cache identities.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    /// Read-only source image for embedding in the runtime owner's fixed source.
    #[must_use]
    pub fn projection(&self) -> serde_json::Value {
        let stages: Vec<_> = self.stages.iter().map(|stage| serde_json::json!({
            "name": stage.name(), "source": stage.source(), "source_sha256": stage.source_sha256(), "arguments": stage.arguments(),
        })).collect();
        serde_json::json!({
            "schema": 1, "purpose": "source-intent-cold-sdk", "role": "root-linux", "host": self.root.host().target_triple(),
            "rust_version": self.root.compiler_version(), "toolchain": format!("{}-{}", self.root.compiler_version(), self.root.host().target_triple()),
            "namespace": self.root.relative_to_runner_temp(),
            "leaves": self.root.leaves().iter().map(|leaf| leaf.relative()).collect::<Vec<_>>(),
            "stages": stages, "environment": self.environment,
            "mise_sha256": self.acquisition.distribution().binary_sha256(), "manager_sha256": self.root.host().sha256(),
            "mise_qualification_sha256": qualified_toolset_digest(std::slice::from_ref(self.acquisition.distribution())),
            "installer_source_identity": velnor_actions_contract::compiled_source_sha256(self.stages.iter().map(SourceIntentInstallStage::source).collect::<Vec<_>>().join("\0").as_bytes()),
            "compiler_source_authority": self.compiler.projection(),
            "expected_obligations": {
                "profile": "minimal",
                "components": ["cargo", "rustc", "rust-std", "clippy", "rustfmt"],
                "targets": [self.root.host().target_triple()],
                "commands": ["rustc", "cargo", "rustdoc", "cargo-clippy", "clippy-driver", "rustfmt", "cargo-fmt"],
            },
        })
    }

    /// Rebuild the exact owner image before it crosses into runtime source.
    /// # Errors
    /// Rejects stale qualification or altered source, arguments or environment.
    pub fn verify_fresh(&self) -> Result<(), MiseError> {
        if &Self::root_linux(&self.version)? != self {
            return Err(MiseError::Contract {
                problem: "source_intent_installer_binding_changed".to_owned(),
            });
        }
        Ok(())
    }
}

fn canonical_environment(
    root: SourceIntentColdRoot,
    acquisition_environment: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut environment: BTreeMap<_, _> = crate::command::ISOLATION_ENV
        .into_iter()
        .chain(crate::command::NO_AUTO_INSTALL_ENV)
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    environment.extend(acquisition_environment.clone());
    environment.extend([
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        (
            "CARGO_HOME".to_owned(),
            root.leaf_expression(SourceIntentColdLeaf::Cargo),
        ),
        (
            "RUSTUP_HOME".to_owned(),
            root.leaf_expression(SourceIntentColdLeaf::Rustup),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            root.leaf_expression(SourceIntentColdLeaf::Cargo),
        ),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            root.leaf_expression(SourceIntentColdLeaf::Rustup),
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
        (
            "MISE_CONFIG_DIR".to_owned(),
            root.leaf_expression(SourceIntentColdLeaf::MiseConfig),
        ),
        (
            "MISE_SYSTEM_CONFIG_DIR".to_owned(),
            root.leaf_expression(SourceIntentColdLeaf::MiseSystemConfig),
        ),
    ]);
    environment
}

fn clear_source(root: SourceIntentColdRoot, version: &str) -> Result<String, MiseError> {
    let (key, _) = root.namespace_environment();
    let body = format!(
        "set -euo pipefail\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\n/usr/bin/python3 -I -S - <<'VELNOR_SOURCE_INTENT_CLEAR'\n{}\nROOT_RELATIVE = {:?}\nROOT_ENV = {:?}\n{}\nclear_source_intent()\nVELNOR_SOURCE_INTENT_CLEAR\n",
        include_str!("catalog_native_health.py"),
        root.relative_to_runner_temp(),
        key,
        include_str!("catalog_source_intent_clear.py"),
    );
    velnor_actions_contract::generated_source(version, &body).map_err(|error| MiseError::Contract {
        problem: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unqualified_native_compiler_blocks_before_mise_or_installation() {
        let result = SourceIntentColdInstaller::root_linux("0.1.0");
        assert!(matches!(result, Err(MiseError::Contract { problem })
            if problem.contains("root Linux Rust compiler artifact authority absent")));
    }
}
