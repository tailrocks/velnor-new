//! Source-bound generated helper references and compiled owner records.
use crate::ContractError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[path = "compiler_execution.rs"]
mod compiler;
#[path = "source_helper_metadata.rs"]
mod metadata;
#[path = "source_helper_operation.rs"]
mod operation;
#[path = "source_helper_output.rs"]
mod output;
pub use compiler::{
    CompiledRustReportRecipe, CompilerDriver, RustCompilerOperation, RustCompilerTools,
    RustReportFrame, quote_literal_run_arg,
};
pub use operation::SourceBoundOperation;

/// Transport bound for complete argument-vector bytes; owner records may exceed it.
/// The renderer returns typed unsupported admission before serializing those bytes.
pub const SOURCE_HELPER_ARGUMENT_BYTES_MAX: usize = 524_288;

#[cfg(test)]
#[path = "source_helper_hash_tests.rs"]
mod hash_tests;

#[cfg(test)]
#[path = "source_helper_snapshot_tests.rs"]
mod snapshot_tests;

/// SHA-256 identity of complete immutable compiler-owned source bytes.
#[must_use]
pub fn compiled_source_sha256(source: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(source) {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Wire descriptor. Valid shape alone never grants execution authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBoundHelper {
    operation: SourceBoundOperation,
    path: String,
    source_sha256: String,
}

impl SourceBoundHelper {
    /// Construct a reference from a compiled owner's source digest.
    /// # Errors
    /// Rejects an unowned path or malformed digest.
    pub fn compiled(
        operation: SourceBoundOperation,
        path: &str,
        sha256: &str,
    ) -> Result<Self, ContractError> {
        let value = Self {
            operation,
            path: path.to_owned(),
            source_sha256: sha256.to_owned(),
        };
        value.validate()?;
        Ok(value)
    }

    /// Validate shape; renderer separately requires exact compiled-record admission.
    /// # Errors
    /// Rejects an unowned path or malformed digest.
    pub fn validate(&self) -> Result<(), ContractError> {
        let expected_path = match self.operation {
            SourceBoundOperation::NativeSwiftExecution
            | SourceBoundOperation::NativeRustExecution
            | SourceBoundOperation::RustReportWrapper => {
                format!("{}{}.sh", self.operation.path(), self.source_sha256)
            }
            _ => self.operation.path().to_owned(),
        };
        if self.path != expected_path || !crate::ids::is_lower_hex_len(&self.source_sha256, 64) {
            return Err(ContractError::identity(
                "source_helper",
                "invalid_source_binding",
            ));
        }
        Ok(())
    }

    /// Qualified operation.
    #[must_use]
    pub const fn operation(&self) -> SourceBoundOperation {
        self.operation
    }
    /// Owned generated path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Digest of complete emitted source bytes, including its marker.
    #[must_use]
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
}

/// Owner-qualified argument vector and managed-tool requirements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperInvocation {
    helper: SourceBoundHelper,
    args: Vec<String>,
    installed_selectors: Vec<String>,
    execution_prefix: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_validation_descriptor:
        Option<Box<super::native_validation_descriptor::NativeValidationDescriptor>>,
}

impl HelperInvocation {
    /// Construct an invocation after source owner validates argument semantics.
    /// # Errors
    /// Rejects malformed descriptors, empty arguments, or control characters.
    pub fn compiled(
        helper: SourceBoundHelper,
        args: Vec<String>,
        installed_selectors: Vec<String>,
    ) -> Result<Self, ContractError> {
        let value = Self {
            helper,
            args,
            installed_selectors,
            execution_prefix: Vec::new(),
            native_validation_descriptor: None,
        };
        value.validate()?;
        Ok(value)
    }
    /// Validate structural shape; semantic authority stays with the compiled owner.
    /// # Errors
    /// Rejects malformed descriptors, empty arguments, or control characters.
    pub fn validate(&self) -> Result<(), ContractError> {
        self.helper.validate()?;
        if matches!(
            self.helper.operation(),
            SourceBoundOperation::RustReleaseSourceSnapshot
                | SourceBoundOperation::RustReleasePreparedPackage
                | SourceBoundOperation::RustReleasePackageVerify
        ) && !self.args.is_empty()
        {
            return Err(ContractError::identity(
                "source_helper",
                "snapshot_arguments",
            ));
        }
        if let Some(descriptor) = self.native_validation_descriptor.as_deref() {
            descriptor.validate()?;
            let operation = match descriptor {
                super::native_validation_descriptor::NativeValidationDescriptor::PackageUpdateFixture { .. } => SourceBoundOperation::PackageUpdateFixture,
                super::native_validation_descriptor::NativeValidationDescriptor::HomebrewPreparation { .. } => SourceBoundOperation::HomebrewPreparation,
            };
            if self.helper.operation() != operation {
                return Err(ContractError::identity(
                    "source_helper",
                    "native_descriptor_operation",
                ));
            }
        }
        if self.args.len() > 2048
            || self.installed_selectors.len() > 2048
            || self.execution_prefix.len() > 2048
            || self.execution_prefix.iter().map(String::len).sum::<usize>()
                > SOURCE_HELPER_ARGUMENT_BYTES_MAX
            || (!self.execution_prefix.is_empty()
                && self
                    .execution_prefix
                    .last()
                    .is_none_or(|value| value != "--"))
            || self
                .args
                .iter()
                .chain(&self.installed_selectors)
                .chain(&self.execution_prefix)
                .any(|arg| arg.is_empty() || arg.chars().any(char::is_control))
        {
            return Err(ContractError::identity("source_helper", "invalid_argument"));
        }
        Ok(())
    }
    /// Source reference.
    #[must_use]
    pub const fn descriptor(&self) -> &SourceBoundHelper {
        &self.helper
    }
    /// Complete owner-validated argument vector.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }
    /// Managed tool selectors installed by this invocation, supplied by its owner.
    #[must_use]
    pub fn installed_selectors(&self) -> &[String] {
        &self.installed_selectors
    }
    /// Fixed tool launcher supplied by the compiled SDK owner.
    #[must_use]
    pub fn execution_prefix(&self) -> &[String] {
        &self.execution_prefix
    }
    /// Semantic metadata; never execution authority by itself.
    #[must_use]
    pub fn native_validation_descriptor(
        &self,
    ) -> Option<&super::native_validation_descriptor::NativeValidationDescriptor> {
        self.native_validation_descriptor.as_deref()
    }
}

/// Compiled source owner's authority record. Never deserialized from user data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSourceHelper {
    invocation: HelperInvocation,
    source: String,
    environment: BTreeMap<String, String>,
    execution_recipe: Option<super::native_tools::CompiledNativeExecRecipe>,
    github_output: bool,
    compiler_binding: Option<compiler::CompiledCompilerBinding>,
}

impl CompiledSourceHelper {
    /// Register source produced by a compiled owner factory.
    /// # Errors
    /// Rejects invalid invocation or missing generated marker.
    pub fn compiled(invocation: HelperInvocation, source: String) -> Result<Self, ContractError> {
        invocation.validate()?;
        if compiled_source_sha256(source.as_bytes()) != invocation.descriptor().source_sha256() {
            return Err(ContractError::identity("source_helper", "source_digest"));
        }
        if source.len() > 262_144
            || !source
                .lines()
                .next()
                .is_some_and(crate::is_generated_marker_line)
            || source.contains('\0')
        {
            return Err(ContractError::identity(
                "source_helper",
                "invalid_compiled_source",
            ));
        }
        Ok(Self {
            invocation,
            source,
            environment: BTreeMap::new(),
            execution_recipe: None,
            github_output: false,
            compiler_binding: None,
        })
    }
    /// Bind the exact environment validated by the source operation's owner.
    #[must_use]
    pub fn with_environment(mut self, environment: BTreeMap<String, String>) -> Self {
        self.environment = environment;
        self
    }
    /// Admit the runner output channel for a closed producer reconstructed by its owner.
    /// # Errors
    /// Rejects foreign operations or a user-supplied output-path environment.
    pub fn with_github_output(mut self) -> Result<Self, ContractError> {
        if !output::eligible(&self.invocation) || self.environment.contains_key("GITHUB_OUTPUT") {
            return Err(ContractError::identity(
                "source_helper",
                "output_capability",
            ));
        }
        self.github_output = true;
        Ok(self)
    }
    /// Whether the compiled producer may use the runner's existing output channel.
    #[must_use]
    pub const fn github_output(&self) -> bool {
        self.github_output
    }
    /// Bind an SDK-qualified tool launcher to the complete source invocation.
    /// # Errors
    /// Rejects inconsistent tool requirements or conflicting owned environment.
    pub fn with_execution_recipe(
        mut self,
        recipe: super::native_tools::CompiledNativeExecRecipe,
    ) -> Result<Self, ContractError> {
        recipe.validate()?;
        if recipe.is_homebrew_foundation()
            != (self.invocation.descriptor().operation()
                == SourceBoundOperation::HomebrewPreparation)
        {
            return Err(ContractError::identity(
                "source_helper",
                "foundation_operation",
            ));
        }
        if !self.invocation.installed_selectors.is_empty()
            && self.invocation.installed_selectors != recipe.installed_selectors()
        {
            return Err(ContractError::identity("source_helper", "recipe_selectors"));
        }
        for (key, value) in recipe.environment() {
            if self
                .environment
                .get(key)
                .is_some_and(|existing| existing != value)
            {
                return Err(ContractError::identity(
                    "source_helper",
                    "recipe_environment",
                ));
            }
            self.environment.insert(key.clone(), value.clone());
        }
        self.invocation.installed_selectors = recipe.installed_selectors().to_vec();
        self.invocation.execution_prefix = recipe.prefix().to_vec();
        self.invocation.validate()?;
        self.execution_recipe = Some(recipe);
        Ok(self)
    }
    /// Compiled SDK execution authority, never reconstructed from wire data.
    #[must_use]
    pub const fn execution_recipe(&self) -> Option<&super::native_tools::CompiledNativeExecRecipe> {
        self.execution_recipe.as_ref()
    }
    /// Verify nonserialized execution authority still agrees with wire metadata.
    /// # Errors
    /// Rejects edited prefixes, requirements, or recipe environment.
    pub fn validate_binding(&self) -> Result<(), ContractError> {
        compiler::validate(self)?;
        self.invocation.validate()?;
        if compiled_source_sha256(self.source.as_bytes())
            != self.invocation.descriptor().source_sha256()
        {
            return Err(ContractError::identity("source_helper", "source_digest"));
        }
        output::validate(self)?;
        match &self.execution_recipe {
            Some(recipe) => {
                recipe.validate()?;
                if recipe.is_homebrew_foundation()
                    != (self.invocation.descriptor().operation()
                        == SourceBoundOperation::HomebrewPreparation)
                {
                    return Err(ContractError::identity(
                        "source_helper",
                        "foundation_operation",
                    ));
                }
                if recipe.prefix() != self.invocation.execution_prefix()
                    || recipe.installed_selectors() != self.invocation.installed_selectors()
                    || recipe
                        .environment()
                        .iter()
                        .any(|(key, value)| self.environment.get(key) != Some(value))
                {
                    return Err(ContractError::identity("source_helper", "recipe_binding"));
                }
            }
            None if !self.invocation.execution_prefix().is_empty()
                || self.invocation.descriptor().operation()
                    == SourceBoundOperation::HomebrewPreparation =>
            {
                return Err(ContractError::identity("source_helper", "recipe_authority"));
            }
            None => {}
        }
        Ok(())
    }
    /// Environment authorized by the compiled source owner.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
    /// Exact invocation authorized by the source owner.
    #[must_use]
    pub const fn invocation(&self) -> &HelperInvocation {
        &self.invocation
    }
    /// Complete generated source bytes.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}
