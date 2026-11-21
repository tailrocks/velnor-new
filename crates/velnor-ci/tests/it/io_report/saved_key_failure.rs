// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::report::LaneMetrics;

fn failed_save(saved_key: Option<&str>) -> LaneMetrics {
    let mut lane = crate::report::lane();
    lane["invocations"] = json!([]);
    lane["caches"][0]["family"] = json!("tools");
    lane["caches"][0]["requested_key"] = json!("tools-old");
    lane["caches"][0]["save"] = json!("failed");
    if let Some(key) = saved_key {
        lane["caches"][0]["saved_key"] = json!(key);
    }
    serde_json::from_value(lane).unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn a_failed_save_names_the_saved_key_when_it_differs_from_the_requested_key() {
    let violations = velnor_ci::report::waste(&failed_save(Some("tools-new")));
    assert_eq!(violations, ["tools cache save failed: tools-new"]);
}

#[test]
fn a_failed_save_without_a_saved_key_names_the_requested_key() {
    let violations = velnor_ci::report::waste(&failed_save(None));
    assert_eq!(violations, ["tools cache save failed: tools-old"]);
}
