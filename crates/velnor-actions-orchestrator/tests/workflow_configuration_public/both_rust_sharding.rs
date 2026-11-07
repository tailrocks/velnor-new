// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use velnor_actions_contract::{ExecutionMode, WorkflowPolicy};
use velnor_actions_orchestrator::{prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::maintenance_features::owner_feature_repo;

#[test]
fn paired_rust_rejects_sharding_and_single_lanes_remain_available() -> Result<(), Box<dyn Error>> {
    let unsharded = repository(false)?;
    let unsharded_prepared =
        prepare(unsharded.path()).map_err(|error| format!("unsharded prepare: {error:?}"))?;
    let both = render_staged_tree_with(&unsharded_prepared, Some(ExecutionMode::Both))
        .map_err(|error| format!("unsharded Both render: {error:?}"))?;
    let workflow = both.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    assert!(workflow.contains("rust-maintenance__hosted:"));
    assert!(workflow.contains("rust-maintenance__local:"));

    let sharded = repository(true)?;
    let prepared =
        prepare(sharded.path()).map_err(|error| format!("sharded prepare: {error:?}"))?;
    let partitions: Vec<_> = prepared
        .discovery
        .proposals
        .iter()
        .filter(|task| {
            task.stack_id == "rust"
                && task.task_kind == "nextest"
                && task.identity.unit_path == "crates/maintenance/Cargo.toml"
        })
        .filter_map(|task| {
            velnor_actions_contract::split_shard_suffix(&task.task_id).map(|(_, _, count)| count)
        })
        .collect();
    assert_eq!(partitions, [2, 2]);

    for mode in [ExecutionMode::Hosted, ExecutionMode::ScaleSet] {
        render_staged_tree_with(&prepared, Some(mode))
            .map_err(|error| format!("sharded {mode:?} render: {error:?}"))?;
    }
    let error = render_staged_tree_with(&prepared, Some(ExecutionMode::Both))
        .err()
        .ok_or("paired Rust accepted a multi-shard task")?;
    assert!(
        format!("{error:?}").contains("paired_rust_sharding_unsupported"),
        "unexpected rejection: {error:?}"
    );
    Ok(())
}

fn repository(sharded: bool) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        false,
        "default",
        "default",
    )?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/maintenance\"]\nresolver = \"2\"\n",
    )?;
    fs::remove_dir_all(root.join("src"))?;
    super::workflow_configuration_fixture::write_root_cargo_lock(root, &["maintenance"])?;
    super::workflow_configuration_fixture::add_independent_mbx_package(root, "crates/a-b", "a-b")?;
    super::workflow_configuration_fixture::add_independent_mbx_package(root, "crates/a/c", "c")?;
    let config_path = repo.path().join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?.replace("schema = 1", "schema = 2");
    let shards = if sharded {
        "\n[test_sharding]\ndefault_shards = 1\n\n[test_sharding.by_manifest]\n\"crates/maintenance/Cargo.toml\" = 2\n"
    } else {
        ""
    };
    fs::write(
        config_path,
        format!(
            "{config}{shards}\n[execution]\nmode = \"hosted\"\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n"
        ),
    )?;
    Ok(repo)
}
