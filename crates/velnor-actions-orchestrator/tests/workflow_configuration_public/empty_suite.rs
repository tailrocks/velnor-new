// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{GenerationPreparation, prepare};

use super::maintenance_features::owner_feature_repo;

#[test]
fn prepare_emits_fail_by_default_and_warn_for_unsharded_nextest_when_opted_in()
-> Result<(), Box<dyn Error>> {
    let repo = empty_suite_repo(false)?;
    let root = repo.path();
    let default = prepare(root)?;
    assert_empty_suite_config(&default, false)?;
    let default_tasks = empty_suite_tasks(&default);

    write_empty_suite_config(root, true)?;
    let opted_in = prepare(root)?;
    assert_empty_suite_config(&opted_in, true)?;
    let opted_in_tasks = empty_suite_tasks(&opted_in);

    assert_eq!(
        default_tasks.keys().collect::<BTreeSet<_>>(),
        opted_in_tasks.keys().collect()
    );
    let mut unsharded_nextest = 0;
    let mut sharded_nextest = 0;
    for (task_id, enabled_task) in &opted_in_tasks {
        let default_task = default_tasks
            .get(task_id)
            .ok_or("default task is missing")?;
        assert_eq!(enabled_task.task_kind, default_task.task_kind);
        if enabled_task.task_kind == "nextest" {
            let sharded = enabled_task
                .payload
                .iter()
                .any(|flag| flag == "--partition");
            if sharded {
                sharded_nextest += 1;
                assert_eq!(no_tests_value(&enabled_task.payload), Some("warn"));
                assert_eq!(no_tests_value(&default_task.payload), Some("warn"));
                assert_eq!(enabled_task.payload, default_task.payload, "{task_id}");
            } else {
                unsharded_nextest += 1;
                assert_eq!(no_tests_value(&default_task.payload), Some("fail"));
                assert_eq!(no_tests_value(&enabled_task.payload), Some("warn"));
                let mut enabled_payload = enabled_task.payload.clone();
                flip_no_tests(&mut enabled_payload, "warn", "fail")?;
                assert_eq!(enabled_payload, default_task.payload, "{task_id}");
            }
        } else {
            assert_eq!(enabled_task.payload, default_task.payload, "{task_id}");
            assert_eq!(no_tests_value(&enabled_task.payload), None, "{task_id}");
        }
    }
    assert!(unsharded_nextest > 0);
    assert!(sharded_nextest > 0);
    Ok(())
}

fn empty_suite_repo(allow_empty_test_suite: bool) -> Result<TempDir, Box<dyn Error>> {
    let repo = owner_feature_repo(WorkflowPolicy::ConsumerV1, false, "empty-suite", "default")?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"maintenance\"\nversion = \"0.1.0\"\n",
    )?;
    write_empty_suite_config(root, allow_empty_test_suite)?;
    Ok(repo)
}

fn write_empty_suite_config(
    root: &Path,
    allow_empty_test_suite: bool,
) -> Result<(), Box<dyn Error>> {
    let empty_suite_setting = if allow_empty_test_suite {
        "allow_empty_test_suite = true\n"
    } else {
        ""
    };
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[test_sharding]\ndefault_shards = 1\n\n[test_sharding.by_manifest]\n\"crates/maintenance/Cargo.toml\" = 2\n\n[stacks.rust]\ntest_runner = \"cargo_nextest\"\n{empty_suite_setting}\n[[stacks.rust.configurations]]\nname = \"empty-suite\"\nfeatures = [\"default\"]\nall_features = false\ntarget = \"host\"\n"
    );
    fs::write(root.join(".velnor/config.toml"), config)?;
    Ok(())
}

fn assert_empty_suite_config(
    prepared: &GenerationPreparation,
    expected_allow_empty_test_suite: bool,
) -> Result<(), Box<dyn Error>> {
    let rust = prepared
        .config
        .stacks
        .rust
        .as_ref()
        .ok_or("Rust config missing")?;
    assert_eq!(rust.allow_empty_test_suite, expected_allow_empty_test_suite);
    Ok(())
}

fn empty_suite_tasks(
    prepared: &GenerationPreparation,
) -> BTreeMap<&str, &velnor_actions_contract::ProposedTask> {
    prepared
        .discovery
        .proposals
        .iter()
        .filter(|task| {
            task.stack_id == "rust"
                && task.configuration == "empty-suite"
                && !task.identity.unit_id.is_empty()
        })
        .map(|task| (task.task_id.as_str(), task))
        .collect()
}

fn no_tests_value(payload: &[std::ffi::OsString]) -> Option<&str> {
    payload
        .iter()
        .position(|argument| argument == "--no-tests")
        .and_then(|index| payload.get(index + 1))
        .and_then(|value| value.to_str())
}

fn flip_no_tests(
    payload: &mut [std::ffi::OsString],
    from: &str,
    to: &str,
) -> Result<(), Box<dyn Error>> {
    let index = payload
        .iter()
        .position(|argument| argument == "--no-tests")
        .ok_or("nextest payload carries no --no-tests")?;
    let value = payload
        .get(index + 1)
        .ok_or("nextest payload truncates its --no-tests value")?;
    assert_eq!(value, from);
    payload[index + 1] = std::ffi::OsString::from(to);
    Ok(())
}
