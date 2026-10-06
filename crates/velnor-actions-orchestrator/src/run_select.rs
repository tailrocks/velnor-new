//! Exact-base candidate and baseline-artifact selection (PAR-5.5).
//!
//! Run summaries discover candidates. Immutable baseline evidence is
//! authenticated against its exact artifact and original attempt record.

/// One selected exact-base run: run plus its successful attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedBaseRun {
    /// Numeric GitHub run ID.
    pub run_id: u64,
    /// Successful attempt the baseline manifest must claim.
    pub attempt: u64,
}

/// Service metadata for one exact, unexpired artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineArtifactMetadata {
    /// Service-assigned numeric artifact ID used by the ZIP endpoint.
    pub id: u64,
    /// Downloaded ZIP size in bytes.
    pub size_in_bytes: u64,
    /// GitHub service digest (`sha256:<64 lowercase hex>`).
    pub digest: String,
}

/// Select the newest current successful push run for analysis evidence.
///
/// The service lists newest first, so the first entry matching the base
/// SHA, branch, push event, success conclusion, and a positive recorded
/// attempt wins. This does not authenticate an immutable baseline: baseline
/// lookup uses [`select_exact_base_candidates`] and the attempt API record.
///
/// # Errors
///
/// Returns `baseline_unavailable` unless at least one run matches.
pub fn select_exact_base_run(
    text: &str,
    base: &str,
    branch: &str,
) -> Result<SelectedBaseRun, String> {
    let runs: Vec<serde_json::Value> =
        serde_json::from_str(text).map_err(|_| "baseline_unavailable".to_owned())?;
    runs.iter()
        .filter(|run| {
            run["headSha"] == base
                && run["headBranch"] == branch
                && run["event"] == "push"
                && run["conclusion"] == "success"
        })
        .find_map(|run| {
            let run_id = run["databaseId"].as_u64().filter(|id| *id > 0)?;
            let attempt = run["attempt"].as_u64().filter(|n| *n > 0)?;
            Some(SelectedBaseRun { run_id, attempt })
        })
        .ok_or_else(|| "baseline_unavailable".to_owned())
}

/// Discover bounded exact-base push runs in the service's newest-first order.
///
/// Summary conclusions confer no proof authority. A failed newer retry can
/// retain an independently authenticated successful original publication.
/// # Errors
/// Refuses malformed or oversized listings and listings without candidates.
pub fn select_exact_base_candidates(
    text: &str,
    base: &str,
    branch: &str,
) -> Result<Vec<u64>, String> {
    let value = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let runs = value
        .as_array()
        .filter(|runs| runs.len() <= 50)
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    let mut seen = std::collections::BTreeSet::new();
    let candidates: Vec<_> = runs
        .iter()
        .filter(|run| {
            run["headSha"] == base
                && run["headBranch"] == branch
                && run["event"] == "push"
                && run["attempt"].as_u64().is_some_and(|attempt| attempt > 0)
        })
        .filter_map(|run| {
            run["databaseId"]
                .as_u64()
                .filter(|id| *id > 0 && seen.insert(*id))
        })
        .collect();
    if candidates.is_empty() {
        Err("baseline_unavailable".to_owned())
    } else {
        Ok(candidates)
    }
}

/// Select the exact baseline artifact from a run-artifacts listing.
///
/// Requires the complete `gh api` object shape with `total_count`. The entry
/// must name `expected` exactly and carry an explicit
/// `"expired": false`; a missing, expired, neighbouring, or
/// expiry-unattested (absent/null/non-bool `expired`) artifact never
/// selects.
///
/// # Errors
///
/// Returns `baseline_unavailable` unless exactly such an entry exists.
pub fn select_baseline_artifact(text: &str, expected: &str) -> Result<u64, String> {
    select_baseline_artifact_metadata(text, expected).map(|artifact| artifact.id)
}

/// Select exact artifact metadata required to authenticate the ZIP bytes.
///
/// Requires one complete listing and exactly one exact-name, unexpired entry
/// with a positive ID, size, and canonical SHA-256 digest.
/// # Errors
pub fn select_baseline_artifact_metadata(
    text: &str,
    expected: &str,
) -> Result<BaselineArtifactMetadata, String> {
    let value = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let entries = value.get("artifacts").and_then(serde_json::Value::as_array);
    let Some(entries) = entries else {
        return Err("baseline_unavailable".to_owned());
    };
    if value["total_count"].as_u64() != u64::try_from(entries.len()).ok() {
        return Err("baseline_unavailable".to_owned());
    }
    let mut matching = entries
        .iter()
        .filter(|entry| entry["name"] == expected && entry["expired"].as_bool() == Some(false));
    let entry = matching
        .next()
        .filter(|_| matching.next().is_none())
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    let id = entry["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    let size_in_bytes = entry["size_in_bytes"]
        .as_u64()
        .filter(|size| *size > 0)
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    let digest = entry["digest"]
        .as_str()
        .filter(|digest| valid_service_digest(digest))
        .ok_or_else(|| "baseline_unavailable".to_owned())?
        .to_owned();
    Ok(BaselineArtifactMetadata {
        id,
        size_in_bytes,
        digest,
    })
}

/// GitHub artifact digest is lowercase `sha256:` plus 64 hex digits.
fn valid_service_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_base_candidates_do_not_treat_latest_summary_as_proof() {
        let base = "a".repeat(40);
        let runs = serde_json::json!([
            {"databaseId": 7, "headSha": base, "headBranch": "main",
                "event": "push", "conclusion": "failure", "attempt": 2},
            {"databaseId": 7, "headSha": base, "headBranch": "main",
                "event": "push", "conclusion": "success", "attempt": 2},
            {"databaseId": 8, "headSha": base, "headBranch": "main",
                "event": "push", "conclusion": "success", "attempt": 1},
            {"databaseId": 9, "headSha": base, "headBranch": "main",
                "event": "pull_request", "conclusion": "success", "attempt": 1},
            {"databaseId": 10, "headSha": base, "headBranch": "other",
                "event": "push", "conclusion": "success", "attempt": 1},
            {"databaseId": 11, "headSha": base, "headBranch": "main",
                "event": "push", "conclusion": "success", "attempt": 0}
        ]);
        assert_eq!(
            select_exact_base_candidates(&runs.to_string(), &base, "main"),
            Ok(vec![7, 8])
        );
    }

    fn artifact_listing(artifacts: &[serde_json::Value]) -> serde_json::Value {
        let total_count = u64::try_from(artifacts.len()).expect("test count");
        serde_json::json!({"total_count": total_count, "artifacts": artifacts})
    }

    /// Only an explicit `"expired": false` selects; absent, null, true,
    /// and non-bool markers never do.
    #[test]
    fn artifact_expiry_must_be_explicitly_false() {
        let listed = artifact_listing(&[
            serde_json::json!({"id": 10, "name": "n", "expired": false, "size_in_bytes": 1,
             "digest": format!("sha256:{}", "a".repeat(64))}),
        ]);
        assert_eq!(select_baseline_artifact(&listed.to_string(), "n"), Ok(10));
        let listed_with_old_expired = artifact_listing(&[
            serde_json::json!({"id": 9, "name": "n", "expired": true, "size_in_bytes": 4,
             "digest": format!("sha256:{}", "b".repeat(64))}),
            serde_json::json!({"id": 10, "name": "n", "expired": false, "size_in_bytes": 4,
             "digest": format!("sha256:{}", "a".repeat(64))}),
        ]);
        assert_eq!(
            select_baseline_artifact_metadata(&listed_with_old_expired.to_string(), "n")
                .map(|artifact| artifact.id),
            Ok(10)
        );
        for (label, entry) in [
            (
                "absent",
                serde_json::json!({"id": 11, "name": "n", "size_in_bytes": 1,
                "digest": format!("sha256:{}", "a".repeat(64))}),
            ),
            (
                "null",
                serde_json::json!({"id": 12, "name": "n", "expired": null,
                    "size_in_bytes": 1, "digest": format!("sha256:{}", "a".repeat(64))}),
            ),
            (
                "true",
                serde_json::json!({"id": 13, "name": "n", "expired": true,
                    "size_in_bytes": 1, "digest": format!("sha256:{}", "a".repeat(64))}),
            ),
            (
                "string",
                serde_json::json!({"id": 14, "name": "n", "expired": "false",
                    "size_in_bytes": 1, "digest": format!("sha256:{}", "a".repeat(64))}),
            ),
            (
                "number",
                serde_json::json!({"id": 15, "name": "n", "expired": 0,
                    "size_in_bytes": 1, "digest": format!("sha256:{}", "a".repeat(64))}),
            ),
        ] {
            let listed = artifact_listing(&[entry]);
            assert!(
                select_baseline_artifact(&listed.to_string(), "n").is_err(),
                "{label} expiry must never select"
            );
        }
    }

    #[test]
    fn artifact_service_id_digest_and_size_are_required() {
        let valid = serde_json::json!({"id": 10, "name": "n", "expired": false,
            "size_in_bytes": 42, "digest": format!("sha256:{}", "a".repeat(64))});
        assert_eq!(
            select_baseline_artifact_metadata(
                &artifact_listing(std::slice::from_ref(&valid)).to_string(),
                "n"
            ),
            Ok(BaselineArtifactMetadata {
                id: 10,
                size_in_bytes: 42,
                digest: format!("sha256:{}", "a".repeat(64)),
            })
        );
        assert!(
            select_baseline_artifact_metadata(
                &artifact_listing(&[valid.clone(), valid.clone()]).to_string(),
                "n"
            )
            .is_err()
        );
        for (field, value) in [
            ("size_in_bytes", serde_json::json!(0)),
            ("size_in_bytes", serde_json::json!("42")),
            ("digest", serde_json::json!(null)),
            (
                "digest",
                serde_json::json!(format!("sha256:{}", "A".repeat(64))),
            ),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(
                select_baseline_artifact_metadata(&artifact_listing(&[invalid]).to_string(), "n")
                    .is_err()
            );
        }
        let aliased_id = serde_json::json!({"databaseId": 10, "name": "n", "expired": false,
            "size_in_bytes": 42, "digest": format!("sha256:{}", "a".repeat(64))});
        assert!(
            select_baseline_artifact_metadata(&artifact_listing(&[aliased_id]).to_string(), "n")
                .is_err()
        );
    }
}
