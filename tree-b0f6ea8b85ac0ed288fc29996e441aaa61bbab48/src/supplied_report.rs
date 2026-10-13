use serde_json::{Value, json};

/// Serialize a supplied-document native report without changing lint policy.
pub fn supplied_report(report: &crate::Report) -> Value {
    let crates: Vec<_> = report
        .crate_reports()
        .iter()
        .map(|(name, result)| {
            let lints: Vec<_> = result
                .lint_results
                .iter()
                .map(|lint| {
                    json!({
                        "id": lint.semver_query.id,
                        "effective_required_update": lint.effective_required_update,
                        "effective_lint_level": lint.effective_lint_level,
                        "findings": lint.query_results.len(),
                    })
                })
                .collect();
            json!({
                "name": name,
                "success": result.success(),
                "detected_bump": format!("{:?}", result.detected_bump()),
                "required_bump": result.required_bump(),
                "selected_checks": result.selected_checks,
                "skipped_checks": result.skipped_checks,
                "lints": lints,
            })
        })
        .collect();
    json!({"schema_version": 1, "success": report.success(), "crates": crates})
}
