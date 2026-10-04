//! Read-only admission binds a source to substantive protected CI evidence.
use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use super::{validate_repository, validate_source_sha};
use crate::{RenderError, ambient_shell_step};

/// Generator-owned exact-source package helper in the policy checkout.
pub const SOURCE_VALIDATION_PATH: &str = ".github/velnor/release_source_validation.py";

/// Source authority; Actions resolves only the fixed event expression.
#[derive(Debug, Clone, Copy)]
pub enum SourceBinding<'a> {
    /// Immutable reviewed Rust release plan.
    ApprovedLiteral(&'a str),
    /// Consumer source of a protected native event.
    EventSha,
}

/// Admit only exact-source protected CI with a successful Required job.
///
/// The fixed helper reads API JSON and ZIP members; it never executes artifacts.
/// # Errors
/// Rejects invalid identities or tool argv.
pub fn admission_step(
    repository: &str,
    source: SourceBinding<'_>,
    default_branch: &str,
    argv: Vec<String>,
) -> Result<Step, RenderError> {
    validate_repository(repository)?;
    let source_sha = match source {
        SourceBinding::ApprovedLiteral(sha) => {
            validate_source_sha(sha)?;
            sha
        }
        SourceBinding::EventSha => "${{ github.sha }}",
    };
    let env = BTreeMap::from([
        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
        ("ADMISSION_EVENT_POLICY".to_owned(), "rust".to_owned()),
        ("APPROVED_REPOSITORY".to_owned(), repository.to_owned()),
        ("APPROVED_SOURCE_SHA".to_owned(), source_sha.to_owned()),
        (
            "APPROVED_DEFAULT_BRANCH".to_owned(),
            default_branch.to_owned(),
        ),
    ]);
    ambient_shell_step("Admit protected CI candidate", argv, env)
}
