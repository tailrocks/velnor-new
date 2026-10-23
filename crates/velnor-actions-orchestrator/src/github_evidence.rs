//! Trusted GitHub evidence collection and credential-free local input import.

#[path = "github_evidence_collect.rs"]
mod collect;
#[path = "github_evidence_import.rs"]
mod import;
#[cfg(test)]
#[path = "github_evidence_test_support.rs"]
pub(crate) mod test_support;

/// Internal operation that captures the bounded same-repository snapshot.
pub const GITHUB_EVIDENCE_FETCH_OP: &str = velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION;
/// Internal operation that verifies the downloaded local snapshot bundle.
pub const GITHUB_EVIDENCE_IMPORT_OP: &str =
    velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION;

/// Capture GitHub metadata and original artifact bytes using the job's exact read token.
/// # Errors
pub fn run_github_evidence_fetch() -> Result<(), crate::OrchestratorError> {
    collect::run()
}

/// Verify the same-run snapshot bundle and publish it as a fixed local task input.
/// # Errors
pub fn run_github_evidence_import() -> Result<(), crate::OrchestratorError> {
    import::run()
}
