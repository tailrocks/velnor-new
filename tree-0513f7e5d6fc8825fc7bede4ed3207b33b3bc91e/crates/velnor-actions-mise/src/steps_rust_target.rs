//! Typed `Prepare Rust target` step request for cross-compile legs.
//!
//! One fixed `rustup target add` under the pinned Rust tool: the target
//! std lands on the exact host-qualified toolchain that `mise install`
//! created, never on an ambient default. The request shares the install
//! step's default `RUSTUP_HOME`, so it carries argv only, no owned-homes
//! env. Renderers serialize, never invent.

use std::ffi::OsString;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::mise_argv_tail;
use crate::error::MiseError;
use crate::steps::FORBIDDEN_ACKNOWLEDGED_RUSTUP;

/// Contract-fixed display name of the Rust-target prepare step.
/// Emitters use this const, never a retyped string.
pub const PREPARE_RUST_TARGET_STEP: &str = "Prepare Rust target";

/// Fixed `rustup target add` for one cross-compile target on one host toolchain.
///
/// The payload addresses the pinned toolchain by its exact host-qualified
/// name, so an ambient default toolchain can never receive the target std.
/// It shares the install step's default `RUSTUP_HOME`: no owned-homes env,
/// so this request carries argv only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRustTarget {
    /// Build-host triple qualifying the pinned toolchain name.
    host: String,
    /// Target triple whose std is installed.
    target: String,
}

impl PrepareRustTarget {
    /// Install `target` std on the pinned toolchain for `host`.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] for an empty triple.
    pub fn new(host: &str, target: &str) -> Result<Self, MiseError> {
        if host.is_empty() {
            return Err(invalid_input("host", host));
        }
        if target.is_empty() {
            return Err(invalid_input("target", target));
        }
        Ok(Self {
            host: host.to_owned(),
            target: target.to_owned(),
        })
    }

    /// Contract-fixed display name of the emitted step.
    #[must_use]
    pub fn step_name() -> &'static str {
        PREPARE_RUST_TARGET_STEP
    }

    /// Build-host triple qualifying the pinned toolchain name.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Target triple whose std is installed.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        let mut argv = vec![OsString::from("mise")];
        argv.extend(mise_argv_tail("exec", &specs, &self.payload(catalog)));
        argv
    }

    /// Fixed payload: `rustup target add --toolchain <rust>-<host> <target>`.
    fn payload(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        vec![
            OsString::from(FORBIDDEN_ACKNOWLEDGED_RUSTUP),
            OsString::from("target"),
            OsString::from("add"),
            OsString::from("--toolchain"),
            OsString::from(catalog.rust_toolchain_name_for_host(&self.host)),
            OsString::from(&self.target),
        ]
    }
}

fn invalid_input(field: &str, value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}
