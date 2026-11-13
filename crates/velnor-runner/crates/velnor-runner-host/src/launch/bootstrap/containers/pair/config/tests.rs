// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::super::BootstrapContainerRole;
use super::container_memory_limit;
use crate::error::HostError;
use crate::journal::{StorageProvisioningPlan, StorageRole};

const HOME: u64 = 1 << 20;
const WORK: u64 = 2 << 20;
const TMP: u64 = 4 << 20;
const SHM: u64 = 8 << 20;
const RUN: u64 = 16 << 20;
const DATA: u64 = 32 << 20;
const DIND_TMP: u64 = 64 << 20;
const DIND_SHM: u64 = 128 << 20;
const BASE: i64 = 1 << 30;

fn sizes() -> Vec<(StorageRole, u64)> {
    vec![
        (StorageRole::RunnerHome, HOME),
        (StorageRole::RunnerWork, WORK),
        (StorageRole::RunnerTmp, TMP),
        (StorageRole::RunnerShm, SHM),
        (StorageRole::SharedRun, RUN),
        (StorageRole::DindData, DATA),
        (StorageRole::DindTmp, DIND_TMP),
        (StorageRole::DindShm, DIND_SHM),
    ]
}

#[test]
fn runner_limit_is_base_plus_its_in_memory_volumes() -> Result<(), Box<dyn std::error::Error>> {
    let plan = StorageProvisioningPlan::for_test(&sizes());
    let limit = container_memory_limit(&plan, BootstrapContainerRole::Runner, BASE)?;
    assert_eq!(limit, BASE + i64::try_from(HOME + WORK + TMP + SHM + RUN)?);
    Ok(())
}

#[test]
fn dind_limit_is_base_plus_its_in_memory_volumes() -> Result<(), Box<dyn std::error::Error>> {
    let plan = StorageProvisioningPlan::for_test(&sizes());
    let limit = container_memory_limit(&plan, BootstrapContainerRole::Dind, BASE)?;
    assert_eq!(
        limit,
        BASE + i64::try_from(HOME + WORK + RUN + DATA + DIND_TMP + DIND_SHM)?
    );
    Ok(())
}

#[test]
fn plan_missing_a_mounted_role_is_rejected() {
    let mut partial = sizes();
    partial.retain(|(role, _)| *role != StorageRole::RunnerWork);
    let plan = StorageProvisioningPlan::for_test(&partial);
    for role in [BootstrapContainerRole::Runner, BootstrapContainerRole::Dind] {
        assert!(matches!(
            container_memory_limit(&plan, role, BASE),
            Err(HostError::Identity)
        ));
    }
}

#[test]
fn sum_beyond_i64_is_rejected() {
    let plan = StorageProvisioningPlan::for_test(&sizes());
    assert!(matches!(
        container_memory_limit(&plan, BootstrapContainerRole::Runner, i64::MAX),
        Err(HostError::Identity)
    ));
}
