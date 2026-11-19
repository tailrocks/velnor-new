// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::models::{Mount, MountType};

use crate::config::RunnerPlatform;
use crate::error::HostError;
use crate::journal::{StorageProvisioningPlan, StorageRole};
use crate::launch::bootstrap::containers::pair::PairSpecContext;
use crate::worker::{ResourceBudgetConfig, WorkerIdentityNames, WorkerVolumeRole};

use super::runner_create;

#[test]
fn the_local_runner_turns_the_crates_mirror_off_and_keeps_the_production_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let physical: Vec<_> = StorageRole::PHYSICAL
        .iter()
        .map(|role| (*role, 1 << 20))
        .collect();
    let plan = StorageProvisioningPlan::for_test(&physical);
    let names = WorkerIdentityNames::from_nonce(&"a".repeat(32))?;
    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 << 30,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 2 << 30,
        operational_memory_reserve_bytes: 1 << 30,
    }
    .validate()?;
    let context = PairSpecContext {
        plan: &plan,
        names: &names,
        connection_id: "lab",
        budget,
    };
    let runner = runner_create(
        &names,
        format!("sha256:{}", "b".repeat(64)),
        &"c".repeat(64),
        RunnerPlatform::LinuxArm64,
        &context,
        HashMap::new(),
        false,
    )?;
    let env = runner.config.env.unwrap_or_default();
    assert!(env.iter().any(|item| item == "VELNOR_CRATES_MIRROR=off"));
    assert!(env.iter().any(|item| item == "MISE_SKILLS_FETCH=false"));
    assert!(env.iter().any(|item| item.starts_with("DOCKER_HOST=")));
    assert!(
        env.iter()
            .any(|item| item.starts_with("ACTIONS_RUNNER_HOOK_JOB_STARTED="))
    );
    Ok(())
}

fn mounts(local_ci: bool) -> Result<Vec<Mount>, HostError> {
    let plan = super::storage::plan(1, &super::storage::Sizes::default());
    let names = WorkerIdentityNames::from_nonce(&"a".repeat(32))?;
    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 << 30,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 2 << 30,
        operational_memory_reserve_bytes: 1 << 30,
    }
    .validate()?;
    let context = PairSpecContext {
        plan: &plan,
        names: &names,
        connection_id: "velnor-local",
        budget,
    };
    let runner = runner_create(
        &names,
        format!("sha256:{}", "b".repeat(64)),
        &"c".repeat(64),
        RunnerPlatform::LinuxArm64,
        &context,
        HashMap::new(),
        local_ci,
    )?;
    let host = runner.config.host_config.ok_or(HostError::Identity)?;
    assert_eq!(host.binds.unwrap_or_default().len(), 0);
    host.mounts.ok_or(HostError::Identity)
}

#[test]
fn the_local_ci_flag_adds_exactly_one_read_only_metadata_subpath_mount() -> Result<(), HostError> {
    let without = mounts(false)?;
    let with = mounts(true)?;
    assert_eq!(with.len(), without.len() + 1);
    assert_eq!(&with[..without.len()], without.as_slice());
    let extra = &with[without.len()];
    let names = WorkerIdentityNames::from_nonce(&"a".repeat(32))?;
    assert_eq!(extra.typ, Some(MountType::VOLUME));
    assert_eq!(extra.read_only, Some(true));
    assert_eq!(extra.target.as_deref(), Some("/opt/velnor/local"));
    assert_eq!(
        extra.source.as_deref(),
        Some(names.volume_name(WorkerVolumeRole::RunnerMetadata).as_str())
    );
    assert_eq!(
        extra
            .volume_options
            .as_ref()
            .and_then(|o| o.subpath.as_deref()),
        Some("local-ci")
    );
    for mount in &with {
        assert_ne!(mount.typ, Some(MountType::BIND));
    }
    Ok(())
}

#[test]
fn without_the_flag_no_mount_is_under_the_local_directory() -> Result<(), HostError> {
    for mount in mounts(false)? {
        assert!(
            !mount
                .target
                .unwrap_or_default()
                .starts_with("/opt/velnor/local")
        );
    }
    Ok(())
}
