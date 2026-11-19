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

#[test]
fn pair_containers_are_created_for_the_images_platform() -> Result<(), Box<dyn std::error::Error>> {
    use crate::config::RunnerPlatform;
    let physical: Vec<_> = StorageRole::PHYSICAL
        .iter()
        .map(|role| (*role, 1 << 20))
        .collect();
    let plan = StorageProvisioningPlan::for_test(&physical);
    let names = crate::worker::WorkerIdentityNames::from_nonce(&"a".repeat(32))?;
    let budget = crate::worker::ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 << 30,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 2 << 30,
        operational_memory_reserve_bytes: 1 << 30,
    }
    .validate()?;
    let context = super::super::spec::PairSpecContext {
        plan: &plan,
        names: &names,
        connection_id: "lab",
        budget,
    };
    for platform in [RunnerPlatform::LinuxAmd64, RunnerPlatform::LinuxArm64] {
        let body = super::create_body(
            names.private_volume().to_owned(),
            format!("sha256:{}", "b".repeat(64)),
            BootstrapContainerRole::Runner,
            format!("container:{}", "c".repeat(64)),
            platform,
            &context,
        )?;
        assert_eq!(body.options.platform, platform.as_str());
    }
    Ok(())
}

type MountRow = (&'static str, &'static str, bool, Option<&'static str>);
type MountOwned = (String, String, bool, Option<String>);

fn production_names() -> Result<crate::worker::WorkerIdentityNames, HostError> {
    crate::worker::WorkerIdentityNames::from_nonce(&"a".repeat(32))
}

fn mount_rows(
    names: &crate::worker::WorkerIdentityNames,
    role: BootstrapContainerRole,
) -> Result<Vec<MountOwned>, HostError> {
    super::fixed_mounts(names, role)?
        .into_iter()
        .map(|mount| {
            assert_eq!(mount.typ, Some(bollard::models::MountType::VOLUME));
            let subpath = mount
                .volume_options
                .as_ref()
                .and_then(|options| options.subpath.clone());
            Ok((
                mount.source.ok_or(HostError::Identity)?,
                mount.target.ok_or(HostError::Identity)?,
                mount.read_only.ok_or(HostError::Identity)?,
                subpath,
            ))
        })
        .collect()
}

fn expected_rows(
    names: &crate::worker::WorkerIdentityNames,
    rows: &[(StorageRole, MountRow)],
) -> Result<Vec<MountOwned>, HostError> {
    rows.iter()
        .map(|(role, (_, target, read_only, subpath))| {
            Ok((
                super::storage_volume(names, *role)?,
                (*target).to_owned(),
                *read_only,
                subpath.map(str::to_owned),
            ))
        })
        .collect()
}

#[test]
fn production_runner_mounts_are_pinned() -> Result<(), HostError> {
    let names = production_names()?;
    let expected = expected_rows(
        &names,
        &[
            (StorageRole::RunnerHome, ("", "/home/runner", false, None)),
            (
                StorageRole::RunnerHome,
                (
                    "",
                    "/opt/velnor/source-proof/inbox",
                    false,
                    Some("source-proof/inbox"),
                ),
            ),
            (
                StorageRole::RunnerWork,
                ("", "/home/runner/_work", false, None),
            ),
            (StorageRole::SharedRun, ("", "/run", false, None)),
            (
                StorageRole::EngineMetadata,
                (
                    "",
                    "/.velnor-runtime-identity",
                    true,
                    Some("runtime-identity"),
                ),
            ),
            (
                StorageRole::EngineMetadata,
                ("", "/opt/velnor/job-facts", true, Some("job-facts")),
            ),
            (
                StorageRole::EngineMetadata,
                (
                    "",
                    "/opt/velnor/source-proof/control",
                    true,
                    Some("source-proof/control"),
                ),
            ),
        ],
    )?;
    let actual = mount_rows(&names, BootstrapContainerRole::Runner)?;
    assert_eq!(actual, expected);
    assert!(
        actual
            .iter()
            .all(|(_, target, _, _)| !target.starts_with("/opt/velnor/local"))
    );
    Ok(())
}

#[test]
fn production_dind_mounts_are_pinned() -> Result<(), HostError> {
    let names = production_names()?;
    let expected = expected_rows(
        &names,
        &[
            (StorageRole::RunnerHome, ("", "/home/runner", false, None)),
            (
                StorageRole::RunnerWork,
                ("", "/home/runner/_work", false, None),
            ),
            (StorageRole::SharedRun, ("", "/run", false, None)),
            (StorageRole::DindData, ("", "/var/lib/docker", false, None)),
            (
                StorageRole::EngineMetadata,
                ("", "/opt/velnor/job-facts", true, Some("job-facts")),
            ),
            (
                StorageRole::EngineMetadata,
                (
                    "",
                    "/.velnor-runtime-identity",
                    true,
                    Some("runtime-identity"),
                ),
            ),
        ],
    )?;
    assert_eq!(mount_rows(&names, BootstrapContainerRole::Dind)?, expected);
    Ok(())
}

fn spec_hash(role: BootstrapContainerRole) -> Result<String, Box<dyn std::error::Error>> {
    let physical: Vec<_> = StorageRole::PHYSICAL
        .iter()
        .map(|role| (*role, 1 << 20))
        .collect();
    let plan = StorageProvisioningPlan::for_test(&physical);
    let names = production_names()?;
    let budget = crate::worker::ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 << 30,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 2 << 30,
        operational_memory_reserve_bytes: 1 << 30,
    }
    .validate()?;
    let context = super::super::spec::PairSpecContext {
        plan: &plan,
        names: &names,
        connection_id: "lab",
        budget,
    };
    let body = super::create_body(
        names.container_name(match role {
            BootstrapContainerRole::Runner => crate::worker::WorkerContainerRole::Runner,
            BootstrapContainerRole::Dind => crate::worker::WorkerContainerRole::Dind,
        }),
        format!("sha256:{}", "b".repeat(64)),
        role,
        match role {
            BootstrapContainerRole::Runner => format!("container:{}", "c".repeat(64)),
            BootstrapContainerRole::Dind => {
                format!("velnor-{}-private", names.remote_runner_name()?)
            }
        },
        crate::config::RunnerPlatform::LinuxArm64,
        &context,
    )?;
    Ok(crate::launch::bootstrap::containers::create_spec_sha256(
        &body,
    )?)
}

#[test]
fn production_create_spec_hashes_are_pinned() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        spec_hash(BootstrapContainerRole::Runner)?,
        "e601b833f22d0944928658a13f62c1702a7c3b9f78c9635880a45dc105cdfa0e"
    );
    assert_eq!(
        spec_hash(BootstrapContainerRole::Dind)?,
        "5d169cee5b7ab3491a23ebe5ddde5bfd923499129d397f536c414a2e3317fcc1"
    );
    Ok(())
}
