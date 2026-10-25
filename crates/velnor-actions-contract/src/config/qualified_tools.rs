use super::CheckPlatform;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

#[path = "qualified_tool_probes.rs"]
pub(super) mod probes;
#[path = "qualified_tool_recipe.rs"]
mod recipe;
#[path = "qualified_tool_recipe_validation.rs"]
mod recipe_validation;
#[path = "qualified_tool_rustfmt_probes.rs"]
mod rustfmt_probes;
#[path = "qualified_tool_sources.rs"]
pub(super) mod sources;
#[path = "qualified_tool_validation.rs"]
mod validation;

pub use recipe::{
    QualifiedToolArtifactRecipe, QualifiedToolExecutableRecipe, QualifiedToolRecipe,
    QualifiedToolRecipePlatform,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedTool {
    pub id: String,

    pub backend: QualifiedToolBackend,

    pub version: String,

    pub options: QualifiedToolOptions,

    pub depends_on: Vec<String>,

    pub platforms: Vec<QualifiedToolPlatform>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualifiedToolBackend {
    Core { tool: String },

    Aqua { package: String },

    Cargo { crate_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualifiedToolOptions {
    Default,

    Rust {
        components: Vec<String>,

        targets: Vec<String>,
    },

    Cargo {
        default_features: bool,

        features: Vec<String>,

        installation: QualifiedCargoInstallation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualifiedCargoInstallation {
    Source { source_lock_sha256: String },

    Prebuilt { repository: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolArtifact {
    pub url: String,

    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolPlatform {
    pub platform: CheckPlatform,

    pub artifacts: Vec<QualifiedToolArtifact>,

    pub dependency_artifacts: Vec<QualifiedToolArtifact>,

    pub install_tree_sha256: String,

    pub executables: Vec<QualifiedToolExecutable>,
}

impl From<&QualifiedTool> for QualifiedToolRecipe {
    fn from(tool: &QualifiedTool) -> Self {
        Self {
            id: tool.id.clone(),
            backend: tool.backend.clone(),
            version: tool.version.clone(),
            options: tool.options.clone(),
            depends_on: tool.depends_on.clone(),
            platforms: tool
                .platforms
                .iter()
                .map(|platform| QualifiedToolRecipePlatform {
                    platform: platform.platform,
                    artifacts: platform
                        .artifacts
                        .iter()
                        .map(|artifact| QualifiedToolArtifactRecipe {
                            url: artifact.url.clone(),
                        })
                        .collect(),
                    dependency_artifacts: platform
                        .dependency_artifacts
                        .iter()
                        .map(|artifact| QualifiedToolArtifactRecipe {
                            url: artifact.url.clone(),
                        })
                        .collect(),
                    executables: platform
                        .executables
                        .iter()
                        .map(|executable| QualifiedToolExecutableRecipe {
                            name: executable.name.clone(),
                            path: executable.path.clone(),
                            probe: executable.probe.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolExecutable {
    pub name: String,

    pub path: String,

    pub sha256: String,

    pub probe: QualifiedToolProbe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualifiedToolProbe {
    Version {
        expected: String,
    },

    VersionSubcommand {
        expected: String,
    },

    #[serde(rename = "shellcheck_version")]
    ShellCheckVersion {
        expected: String,
    },

    RustcVerbose {
        expected: String,
    },

    RustfmtVersion {
        expected: String,
    },

    CargoNextestVersion {
        expected: String,
    },
}

impl QualifiedTool {
    #[must_use]
    pub fn requires_compiler(&self) -> bool {
        matches!(&self.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
            || matches!(
                &self.options,
                QualifiedToolOptions::Cargo {
                    installation: QualifiedCargoInstallation::Source { .. },
                    ..
                }
            )
            || self.platforms.iter().any(|platform| {
                platform.executables.iter().any(|executable| {
                    matches!(executable.name.as_str(), "cargo" | "rustc")
                        || matches!(
                            executable.probe,
                            QualifiedToolProbe::CargoNextestVersion { .. }
                                | QualifiedToolProbe::RustcVerbose { .. }
                        )
                })
            })
    }

    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        validation::validate_tool(self, file, key)
    }
}

pub fn validate_qualified_tools(tools: &[QualifiedTool], file: &str) -> Result<(), ContractError> {
    validation::validate_registry(tools, file)
}

pub fn validate_qualified_tool_recipe(
    recipe: &QualifiedToolRecipe,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    recipe_validation::validate_recipe(recipe, file, key)
}

pub fn validate_qualified_tool_recipes(
    recipes: &[QualifiedToolRecipe],
    file: &str,
) -> Result<(), ContractError> {
    recipe_validation::validate_registry(recipes, file)
}
