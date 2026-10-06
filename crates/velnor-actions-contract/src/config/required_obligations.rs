//! Native-only expected obligations; unsupported families and phases reject decode.

use super::{Utf8RepoRelDir, WorkloadKind};
use serde::{Deserialize, Serialize};

/// Independently reviewed required native obligation inventory.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredNativeObligations {
    /// Registry schema; currently 1.
    pub schema: u32,
    /// Complete adopted native component requirements, sorted by component.
    pub obligations: Vec<RequiredNativeObligation>,
}

/// One exact component recipe and its complete compiled phase inventory.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredNativeObligation {
    /// Reviewed component name matching a typed workload declaration.
    pub component: String,
    /// Closed native operation; unsupported Rust/Tofu families reject decode.
    pub operation: WorkloadKind,
    /// Repository-relative workload source root.
    pub root: Utf8RepoRelDir,
    /// Complete expected phase inventory; duplicates reject and ordering is immaterial.
    pub phases: Vec<RequiredNativePhase>,
    /// BLAKE3 digest over the complete validated canonical `WorkloadConfig`.
    pub profile_digest: String,
}

macro_rules! phases {
    ($($variant:ident => $id:literal),+ $(,)?) => {
        /// Closed adapter-emitted phase tokens; no arbitrary task kind or command.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
        pub enum RequiredNativePhase {
            $(#[doc = $id] #[serde(rename = $id)] $variant),+
        }
        impl RequiredNativePhase {
            /// Stable emitted phase identity.
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self { $(Self::$variant => $id),+ }
            }
            /// Parse only domain-owned phase spellings.
            #[must_use]
            pub fn from_id(value: &str) -> Option<Self> {
                match value { $($id => Some(Self::$variant)),+, _ => None }
            }
        }
    };
}

phases! {
    Install => "install",
    Lint => "lint",
    Typecheck => "typecheck",
    Check => "check",
    Build => "build",
    Test => "test",
    Syntax => "syntax",
    Shellcheck => "shellcheck",
    Reuse => "reuse",
    Config => "config",
    Alint => "alint",
    Audit => "audit",
    Deny => "deny",
    Graph => "graph",
    Policy => "policy",
    Deps => "deps",
    Package => "package",
    NativeFfi => "native-ffi",
    NativeGenerate => "native-generate",
    NativeXcodeBuild => "native-xcode-build",
    NativeXcodeTest => "native-xcode-test",
    NativeSwiftBuild => "native-swift-build",
    NativeSwiftTest => "native-swift-test",
    HomebrewTapLocal => "homebrew-tap-local",
    HomebrewAudit => "homebrew-audit",
    HomebrewAuditFormula => "homebrew-audit-formula",
    HomebrewAuditCask => "homebrew-audit-cask",
    GradleCheck => "gradle-check",
    PackageUpdateFixtures => "package-update-fixtures",
}

impl RequiredNativeObligations {
    /// Validate closed native requirements without importing task-execution policy.
    /// # Errors
    /// Rejects schema, component, root, digest, duplicate or operation/phase mismatches.
    pub fn validate(&self, file: &str) -> Result<(), crate::ContractError> {
        use std::collections::BTreeSet;
        let fail = |reason| crate::ContractError::config(file, "obligations", reason);
        if self.schema != 1 {
            return Err(fail("unsupported_schema"));
        }
        if self.obligations.is_empty() {
            return Err(fail("empty_registry"));
        }
        let mut previous: Option<&str> = None;
        for entry in &self.obligations {
            if !super::is_valid_workload_name(&entry.component) {
                return Err(fail("invalid_component"));
            }
            if previous.is_some_and(|name| name >= entry.component.as_str()) {
                return Err(fail("components_must_be_sorted_unique"));
            }
            previous = Some(&entry.component);
            Utf8RepoRelDir::parse(entry.root.as_str()).map_err(|_| fail("invalid_root"))?;
            crate::validate_digest(&entry.profile_digest)?;
            let mut phases = BTreeSet::new();
            if entry.phases.is_empty() {
                return Err(fail("empty_phase_inventory"));
            }
            for phase in &entry.phases {
                if !phases.insert(*phase) {
                    return Err(fail("duplicate_phase"));
                }
                if !phase.allowed_for(entry.operation) {
                    return Err(fail("phase_operation_mismatch"));
                }
            }
        }
        Ok(())
    }
}

#[path = "required_obligations_phase.rs"]
mod phase;
