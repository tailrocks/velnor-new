// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, ResultTest, report};
use serde_json::json;

#[test]
fn report_says_why_a_refused_base_cache_was_not_generated() -> ResultTest {
    let repo = Repo::new("report-not-generated");
    let reason = "private dependencies";
    let metrics = report(
        &repo,
        &json!([{"family":"sources","root":null,"restore_step":"restore","save_step":"save","not_generated":reason}]),
        r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#,
    )?;
    assert_eq!(
        metrics.caches[0].save,
        velnor_ci::report::SaveOutcome::NotAttempted
    );
    assert_eq!(metrics.caches[0].not_generated.as_deref(), Some(reason));
    assert!(
        metrics
            .summary()
            .contains("| not generated (private dependencies) |"),
        "{}",
        metrics.summary()
    );
    Ok(())
}
