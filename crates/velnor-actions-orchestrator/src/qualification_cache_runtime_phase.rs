//! Closed wire values for runtime identity observation phases.

/// Phase used to bind a cache owner to measured runner facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualificationCacheRuntimePhase {
    /// Measure the runner before cache import.
    BeforeImport,
    /// Re-measure the runner after cache import.
    AfterImport,
}

impl QualificationCacheRuntimePhase {
    /// Parse the phase value supplied to the runtime-observation CLI operation.
    ///
    /// # Errors
    /// Returns an error when the value is not one of the two closed phase names.
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value {
            "before_import" => Ok(Self::BeforeImport),
            "after_import" => Ok(Self::AfterImport),
            _ => Err("qualification_cache_runtime_phase_invalid"),
        }
    }

    /// Return the canonical environment value emitted by workflow generation.
    #[must_use]
    pub const fn as_env_value(self) -> &'static str {
        match self {
            Self::BeforeImport => "before_import",
            Self::AfterImport => "after_import",
        }
    }
}
