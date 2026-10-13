//! Tofu task-kind spellings shared by closure and (T12) proposals.
//!
//! The contract fixes three task kinds (`Fmt`/`InitForValidate`/
//! `Validate`); the wire spellings are the subcommand-short tokens
//! below. T12 adopts these spellings for task IDs; T10 needs them
//! now so closure fails closed on unknown kinds.

use velnor_actions_contract::ContractError;

/// Tofu task kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TofuTaskKind {
    /// Formatting check over the independent fmt scope.
    Fmt,
    /// Validation-only init (backend-less, readonly lockfile).
    ///
    /// Contract-exact `InitForValidate`; the wire spelling stays
    /// `init` (spec §6.1 `stack/tofu/dir-/init/default` normative).
    InitForValidate,
    /// Real validate in the initialized root.
    Validate,
}

impl TofuTaskKind {
    /// Stable task-id kind segment.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::InitForValidate => "init",
            Self::Validate => "validate",
        }
    }

    /// Parse a kind token; unknown tokens fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for any token outside the three kinds.
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value {
            "fmt" => Ok(Self::Fmt),
            "init" => Ok(Self::InitForValidate),
            "validate" => Ok(Self::Validate),
            _ => Err(ContractError::identity(
                "task_kind",
                format!("unknown_kind:{value}"),
            )),
        }
    }
}
