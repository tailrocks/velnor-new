//! CLI generation warning presentation.

#[path = "../src/dispatch_generate_warnings.rs"]
mod warning_format;

#[test]
fn generate_report_warnings_are_prefixed_for_stderr() {
    let warnings = vec!["retired_tree_cleanup_failed: busy".to_owned()];
    assert_eq!(
        warning_format::warning_lines(&warnings).collect::<Vec<_>>(),
        ["velnor-actions: WARNING: retired_tree_cleanup_failed: busy"]
    );
}
