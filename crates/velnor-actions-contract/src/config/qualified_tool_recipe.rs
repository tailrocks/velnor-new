//! Hash-free, strictly typed metadata for authoring qualified tool records.
use super::{CheckPlatform, QualifiedToolBackend, QualifiedToolOptions, QualifiedToolProbe};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// One qualified installation recipe before measured hashes are attached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolRecipe {
    /// Stable ID referenced by check tool selections.
    pub id: String,
    /// Explicit supported backend and package.
    pub backend: QualifiedToolBackend,
    /// Exact stable numeric version.
    pub version: String,
    /// Typed installation options; never raw flags or environment.
    pub options: QualifiedToolOptions,
    /// Sorted prerequisite tool IDs forming an explicit dependency DAG.
    pub depends_on: Vec<String>,
    /// Sorted per-platform source and executable declarations.
    pub platforms: Vec<QualifiedToolRecipePlatform>,
}

/// One platform-specific source and executable recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolRecipePlatform {
    /// Supported platform, never inferred from runner label.
    pub platform: CheckPlatform,
    /// Complete pinned acquisition URLs for this platform.
    pub artifacts: Vec<QualifiedToolArtifactRecipe>,
    /// Exhaustive pinned dependency source URLs.
    pub dependency_artifacts: Vec<QualifiedToolArtifactRecipe>,
    /// Declared executable identities and fixed version probes.
    pub executables: Vec<QualifiedToolExecutableRecipe>,
}

/// One allowed source URL before its archive digest is measured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolArtifactRecipe {
    /// Exact trusted backend release/source URL.
    pub url: String,
}

/// One declared executable before its installed-byte digest is measured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolExecutableRecipe {
    /// Command name projected onto the task PATH.
    pub name: String,
    /// Normalized POSIX path relative to the owned installation prefix.
    pub path: String,
    /// Closed readonly probe and exact expected output.
    pub probe: QualifiedToolProbe,
}

impl QualifiedToolRecipe {
    /// Validate one recipe's local identity, source, layout, and probe contract.
    /// # Errors
    /// Rejects unsupported backends, platforms, URLs, paths, or probes.
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        super::recipe_validation::validate_recipe(self, file, key)
    }
}
