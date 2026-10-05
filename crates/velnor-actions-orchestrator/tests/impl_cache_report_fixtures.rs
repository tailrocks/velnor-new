//! Cache service-report parsing fixtures.

use super::*;

#[test]
fn service_report_parses_live_shape_for_sequential_runs() {
    // Fixed format sample (live `gh cache list --json` shape); the numbers
    // are illustrative — real totals live in performance.md.
    let body = r#"[{"key":"velnor-v1-sources-x86_64-unknown-linux-gnu-1.98.1-aa","sizeInBytes":17568922},{"key":"mise-tools-v2-typed-runtime-bb","sizeInBytes":65857248}]"#;
    let report =
        cache_trust::summarize_cache_usage(body, 10_737_418_240, 17_568_922, 3).expect("report");
    assert_eq!(report.active_bytes, 17_568_922 + 65_857_248);
    assert_eq!(report.count, 2);
    assert_eq!(report.headroom_bytes, 10_737_418_240 - report.active_bytes);
    assert_eq!(report.aggregate_transfer_bytes, 17_568_922 * 3);
    eprintln!(
        "cache: stored={} transfer={} headroom={} entries={}",
        report.stored_bytes, report.aggregate_transfer_bytes, report.headroom_bytes, report.count
    );
}
