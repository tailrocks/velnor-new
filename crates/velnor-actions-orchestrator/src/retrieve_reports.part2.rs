/// Expected job artifact IDs from a plan value, in plan order.
///
/// Sibling entries share their job's artifact, so the enumeration
/// dedupes: each artifact downloads exactly once while first-seen
/// plan order stays stable.
fn expected_artifact_ids(plan: &serde_json::Value) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(entries) = plan
        .get("matrix")
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_json::Value::as_array)
    {
        for entry in entries {
            if let Some(id) = entry.get("artifact_id").and_then(serde_json::Value::as_str)
                && seen.insert(id)
            {
                ids.push(id);
            }
        }
    }
    ids
}

#[cfg(test)]
#[path = "retrieve_reports_enumeration_tests.rs"]
mod retrieve_reports_enumeration_tests;
#[cfg(test)]
#[path = "retrieve_reports_tests.rs"]
mod retrieve_reports_tests;
