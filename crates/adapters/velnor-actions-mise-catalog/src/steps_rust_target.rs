//! Typed `Prepare Rust target` request for cross-compile legs.
//!
//! The target standard library is installed on the exact host-qualified Rust
//! toolchain that Mise prepared. The request emits argv only and never uses
//! an ambient default toolchain.

use std::ffi::OsString;

use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_mise_core::command::mise_argv_tail;
use velnor_actions_mise_core::error::MiseError;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::steps::FORBIDDEN_ACKNOWLEDGED_RUSTUP;

/// Contract-fixed display name for the Rust-target preparation step.
pub const PREPARE_RUST_TARGET_STEP: &str = "Prepare Rust target";

/// Fixed `rustup target add` request for a cross-compile target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRustTarget {
    host: String,
    target: String,
}

impl PrepareRustTarget {
    /// Install `target` std on the pinned Rust toolchain for `host`.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] unless both triples are
    /// exact targets from the release target catalog. This keeps values such
    /// as `--help` from being interpreted as rustup options.
    pub fn new(host: &str, target: &str) -> Result<Self, MiseError> {
        if ReleaseTarget::parse_triple(host).is_none() {
            return Err(invalid_input("host", host));
        }
        if ReleaseTarget::parse_triple(target).is_none() {
            return Err(invalid_input("target", target));
        }
        Ok(Self {
            host: host.to_owned(),
            target: target.to_owned(),
        })
    }

    /// Contract-fixed display name.
    #[must_use]
    pub fn step_name() -> &'static str {
        PREPARE_RUST_TARGET_STEP
    }

    /// Build-host triple that qualifies the Rust toolchain.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Target triple whose standard library is installed.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Full Mise argv, including the program name.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        let payload = [
            OsString::from(FORBIDDEN_ACKNOWLEDGED_RUSTUP),
            OsString::from("target"),
            OsString::from("add"),
            OsString::from("--toolchain"),
            OsString::from(catalog.rust_toolchain_name_for_host(&self.host)),
            OsString::from(&self.target),
        ];
        let mut argv = vec![OsString::from("mise")];
        argv.extend(mise_argv_tail("exec", &specs, &payload));
        argv
    }
}

fn invalid_input(field: &str, value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}
