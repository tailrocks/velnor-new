//! Generation-only support bytes; these records grant no execution authority.

use crate::{ContractError, generated_source, normalize_posix_path};

/// Complete marked support source supplied by a compiled owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSupportSource {
    path: String,
    source: String,
}

impl CompiledSupportSource {
    /// Bind a generated support path and source to the generator version.
    ///
    /// This emission record carries no interpreter or invocation authority.
    /// # Errors
    /// Rejects unsafe paths, excessive source, NUL bytes, or invalid markers.
    pub fn compiled(path: &str, body: &str, version: &str) -> Result<Self, ContractError> {
        if !path.starts_with(".github/velnor/")
            || normalize_posix_path(path)? != path
            || body.contains('\0')
            || body.len() > 262_144
        {
            return Err(ContractError::identity(
                "compiled_support",
                "invalid_fixed_source",
            ));
        }
        Ok(Self {
            path: path.to_owned(),
            source: generated_source(version, body)?,
        })
    }

    /// Repository-relative generated support path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Complete emitted bytes including the version marker.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}
