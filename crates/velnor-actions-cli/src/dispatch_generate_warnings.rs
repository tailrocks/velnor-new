pub(super) fn warning_lines(warnings: &[String]) -> impl Iterator<Item = String> + '_ {
    warnings
        .iter()
        .map(|warning| format!("velnor-actions: WARNING: {warning}"))
}
