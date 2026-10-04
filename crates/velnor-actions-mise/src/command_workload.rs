//! Native workload directory selection via Mise globals.

use std::ffi::OsString;

use super::IsolatedCommand;
use crate::error::MiseError;

impl IsolatedCommand {
    /// Apply an inspected repository root through Mise's global directory flag.
    ///
    /// # Errors
    /// Returns `InvalidStepInput` for an unsafe root or a non-Mise command.
    pub fn with_mise_root(mut self, root: &str) -> Result<Self, MiseError> {
        crate::catalog::workload::validate_root(root)?;
        if self.program != "mise" {
            return Err(MiseError::InvalidStepInput {
                field: "workload_program".to_owned(),
                value: self.program.to_string_lossy().into_owned(),
            });
        }
        self.args
            .splice(0..0, [OsString::from("--cd"), OsString::from(root)]);
        Ok(self)
    }
}
