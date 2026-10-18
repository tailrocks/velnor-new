//! Plan matrix entries retain lane and adapter metadata.

use super::*;

#[test]
fn entry_cache_ids_recorded_per_lane() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.matrix.include.is_empty());
    let mut lanes = BTreeSet::new();
    for entry in &plan.matrix.include {
        entry.validate("local")?;
        let ids = entry.cache_ids.as_ref().ok_or("cache ids recorded")?;
        ids.validate()?;
        assert!(lanes.insert(ids.lane_id().to_owned()), "lanes distinct");
    }
    let first = plan.matrix.include[0].cache_ids.as_ref().ok_or("ids")?;
    for entry in &plan.matrix.include {
        let ids = entry.cache_ids.as_ref().ok_or("ids")?;
        assert_eq!(ids.toolchain_id(), first.toolchain_id());
        assert_eq!(ids.platform_id(), first.platform_id());
        assert_eq!(ids.cache_format_id(), first.cache_format_id());
        assert_eq!(ids.workspace_id(), first.workspace_id());
    }
    Ok(())
}

#[test]
fn adapter_metadata_forwards_adapter_value() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.matrix.include.is_empty());
    for entry in &plan.matrix.include {
        let meta = entry.adapter_metadata.as_object().ok_or("opaque object")?;
        for key in [
            "package_id",
            "package_name",
            "manifest_key",
            "kind",
            "configuration",
            "target",
            "compile_driver",
            "test_runner",
            "evidence_ids",
            "task_cache_enabled",
            "cargo_target_dir",
        ] {
            assert!(meta.contains_key(key), "{key} in {}", entry.id);
        }
        assert_eq!(meta.len(), 11, "adapter shape only");
        assert_ne!(meta["compile_driver"].as_str().unwrap_or_default(), "");
        assert_ne!(meta["test_runner"].as_str().unwrap_or_default(), "");
        assert!(meta["evidence_ids"].is_array());
    }
    Ok(())
}
