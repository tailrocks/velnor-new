//! Comparison belongs to the cache owner, before transport consumes its bundle.
use crate::{store, workspace_state};
use eyre::{Result, bail};
use mbx_cache_core::CacheDigest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Baseline {
    version: u8,
    pub(super) cache: store::ComparisonState,
    workspace_entries: BTreeMap<String, serde_json::Value>,
    integrity: String,
}

#[derive(Default, Serialize)]
pub(super) struct Delta {
    new_action_results: u64,
    changed_action_results: u64,
    new_predictions: u64,
    new_workspace_variants: u64,
    changed_workspace_variants: u64,
}

impl Delta {
    pub(super) fn useful(&self) -> bool {
        self.new_action_results
            + self.changed_action_results
            + self.new_predictions
            + self.new_workspace_variants
            + self.changed_workspace_variants
            > 0
    }
}

pub(super) fn read(path: &Path) -> Result<Baseline> {
    let baseline: Baseline = serde_json::from_slice(&std::fs::read(path)?)?;
    if baseline.version != 1 || baseline.cache.version != 1 {
        bail!("unsupported cache comparison state version");
    }
    baseline.cache.validate()?;
    workspace_state::validate_semantic_inventory(&baseline.workspace_entries)?;
    if baseline
        .cache
        .attachments
        .keys()
        .any(|name| name != workspace_state::ATTACHMENT)
    {
        bail!("comparison state uses unsupported attachment schemas");
    }

    let bytes = serde_json::to_vec(&(1u8, &baseline.cache, &baseline.workspace_entries))?;
    if baseline.integrity != CacheDigest::blake3(&bytes).hash {
        bail!("cache comparison state integrity mismatch");
    }
    Ok(baseline)
}

pub(super) fn write(path: &Path, root: &Path, cache: &store::ComparisonState) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    if std::fs::canonicalize(parent)?.starts_with(std::fs::canonicalize(root)?) {
        bail!("comparison state must be outside the consumed cache bundle");
    }

    let entries = workspace_state::semantic_inventory(
        root,
        cache.attachments.get(workspace_state::ATTACHMENT),
    )?;
    let integrity = CacheDigest::blake3(&serde_json::to_vec(&(1u8, cache, &entries))?).hash;
    let value = serde_json::json!({"version": 1, "cache": cache, "workspace_entries": entries, "integrity": integrity});
    publish(path, &serde_json::to_vec_pretty(&value)?)
}

fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    std::fs::write(temporary.path(), bytes)?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub(super) fn empty(path: &Path) -> Result<()> {
    let mut baseline = Baseline {
        version: 1,
        cache: store::ComparisonState {
            version: 1,
            action_results: BTreeMap::new(),
            predictions: Default::default(),
            attachments: BTreeMap::new(),
        },
        workspace_entries: BTreeMap::new(),
        integrity: String::new(),
    };
    baseline.integrity = CacheDigest::blake3(&serde_json::to_vec(&(
        1u8,
        &baseline.cache,
        &baseline.workspace_entries,
    ))?)
    .hash;
    publish(path, &serde_json::to_vec_pretty(&baseline)?)
}

pub(super) fn report(
    baseline: Option<&Baseline>,
    root: &Path,
    current: &store::ComparisonState,
) -> Result<(Option<Delta>, String)> {
    let entries = workspace_state::semantic_inventory(
        root,
        current.attachments.get(workspace_state::ATTACHMENT),
    )?;
    let delta = baseline.map(|baseline| compare_inventories(baseline, current, &entries));
    let bytes = serde_json::to_vec(&(1u8, &current.action_results, &current.predictions, entries))?;
    Ok((delta, CacheDigest::blake3(&bytes).hash))
}

fn changes<T: PartialEq>(before: &BTreeMap<String, T>, after: &BTreeMap<String, T>) -> (u64, u64) {
    let mut new = 0;
    let mut changed = 0;
    for (key, value) in after {
        match before.get(key) {
            None => new += 1,
            Some(old) if old != value => changed += 1,
            Some(_) => {}
        }
    }
    (new, changed)
}

fn workspace_variants(
    before: &BTreeMap<String, serde_json::Value>,
    after: &BTreeMap<String, serde_json::Value>,
) -> (u64, u64) {
    let markers = |entries: &BTreeMap<String, serde_json::Value>| {
        entries
            .iter()
            .filter(|(_, value)| {
                value.get("type").and_then(serde_json::Value::as_str) == Some("workspace")
            })
            .map(|(key, _)| key.clone())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let old = markers(before);
    let new = markers(after);
    let mut added = 0;
    let mut changed = 0;
    for key in new.difference(&old) {
        let signature = key.split_once("/state/").map(|(signature, _)| signature);
        if old
            .iter()
            .any(|old| old.split_once("/state/").map(|(signature, _)| signature) == signature)
        {
            changed += 1;
        } else {
            added += 1;
        }
    }
    (added, changed)
}

fn compare_inventories(
    baseline: &Baseline,
    current: &store::ComparisonState,
    entries: &BTreeMap<String, serde_json::Value>,
) -> Delta {
    let (new_action_results, changed_action_results) =
        changes(&baseline.cache.action_results, &current.action_results);
    let (new_workspace_variants, changed_workspace_variants) =
        workspace_variants(&baseline.workspace_entries, entries);
    Delta {
        new_action_results,
        changed_action_results,
        new_predictions: current
            .predictions
            .difference(&baseline.cache.predictions)
            .count() as u64,
        new_workspace_variants,
        changed_workspace_variants,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn cache(actions: &[(&str, u64)], predictions: &[&str]) -> store::ComparisonState {
        store::ComparisonState {
            version: 1,
            action_results: actions
                .iter()
                .map(|(k, v)| (k.to_string(), serde_json::json!({"outputs":v})))
                .collect(),
            predictions: predictions
                .iter()
                .map(|v| v.to_string())
                .collect::<BTreeSet<_>>(),
            attachments: BTreeMap::new(),
        }
    }

    #[test]
    fn comparison_subset_has_no_useful_delta() {
        let baseline = Baseline {
            version: 1,
            integrity: String::new(),
            cache: cache(&[("a", 1), ("b", 2)], &["p", "q"]),
            workspace_entries: BTreeMap::from([("a".into(), serde_json::json!(1))]),
        };
        let delta = compare_inventories(&baseline, &cache(&[("a", 1)], &["p"]), &BTreeMap::new());
        assert!(!delta.useful());
    }

    #[test]
    fn comparison_same_count_replacement_and_changed_results_are_useful() {
        let baseline = Baseline {
            version: 1,
            integrity: String::new(),
            cache: cache(&[("a", 1)], &["p"]),
            workspace_entries: BTreeMap::new(),
        };
        let delta = compare_inventories(&baseline, &cache(&[("a", 2)], &["q"]), &BTreeMap::new());
        assert_eq!(delta.changed_action_results, 1);
        assert_eq!(delta.new_predictions, 1);
        assert!(delta.useful());
        let replacement =
            compare_inventories(&baseline, &cache(&[("b", 1)], &["p"]), &BTreeMap::new());
        assert_eq!(replacement.new_action_results, 1);
        assert!(replacement.useful());
    }
    #[test]
    fn comparison_preserves_nonzero_timing_and_detects_timing_updates() {
        let p = r#"["task",{"adapter":"rustc","payload":"compiler_duration_ns=123"}]"#;
        let q = r#"["task",{"adapter":"rustc","payload":"compiler_duration_ns=456"}]"#;
        let baseline = Baseline {
            integrity: String::new(),
            version: 1,
            cache: cache(&[("a", 1)], &[p]),
            workspace_entries: BTreeMap::new(),
        };
        assert!(
            !compare_inventories(&baseline, &cache(&[("a", 1)], &[p]), &BTreeMap::new()).useful()
        );
        assert_eq!(
            compare_inventories(&baseline, &cache(&[("a", 1)], &[q]), &BTreeMap::new())
                .new_predictions,
            1
        );
    }

    #[test]
    fn baseline_verification_rejects_versions_and_inventory_corruption() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("baseline.json");
        empty(&path)?;
        read(&path)?;
        let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        value["version"] = serde_json::json!(2);
        std::fs::write(&path, serde_json::to_vec(&value)?)?;
        assert!(read(&path).is_err());
        value["version"] = serde_json::json!(1);
        value["workspace_entries"]["unexpected"] = serde_json::json!({"type":"file"});
        std::fs::write(&path, serde_json::to_vec(&value)?)?;
        assert!(read(&path).is_err());
        value["workspace_entries"] = serde_json::json!({});
        value["cache"]["attachments"][workspace_state::ATTACHMENT] =
            serde_json::to_value(CacheDigest::blake3(b"shape-valid attachment corruption"))?;
        std::fs::write(&path, serde_json::to_vec(&value)?)?;
        assert!(
            read(&path)
                .unwrap_err()
                .to_string()
                .contains("integrity mismatch")
        );
        Ok(())
    }
    #[test]
    fn workspace_variant_counts_are_not_rekeyed_file_counts() {
        let marker = serde_json::json!({"type":"workspace"});
        let before = BTreeMap::from([
            ("workspace/sig/state/a".into(), marker.clone()),
            (
                "workspace/sig/state/a/file".into(),
                serde_json::json!({"type":"file"}),
            ),
            ("workspace/sig/state/b".into(), marker.clone()),
        ]);
        let subset = BTreeMap::from([("workspace/sig/state/b".into(), marker.clone())]);
        assert_eq!(workspace_variants(&before, &subset), (0, 0));
        let changed = BTreeMap::from([("workspace/sig/state/c".into(), marker.clone())]);
        assert_eq!(workspace_variants(&before, &changed), (0, 1));
        let added = BTreeMap::from([("workspace/new/state/c".into(), marker)]);
        assert_eq!(workspace_variants(&before, &added), (1, 0));
    }
}
