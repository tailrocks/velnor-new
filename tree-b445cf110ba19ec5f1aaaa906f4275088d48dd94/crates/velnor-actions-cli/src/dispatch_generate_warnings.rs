//! Formatting for generation warnings emitted by the CLI.

/// Format post-publication cleanup warnings for the command's stderr report.
pub(super) fn warning_lines(warnings: &[String]) -> impl Iterator<Item = String> + '_ {
    warnings
        .iter()
        .map(|warning| format!("velnor-actions: WARNING: {warning}"))
}
