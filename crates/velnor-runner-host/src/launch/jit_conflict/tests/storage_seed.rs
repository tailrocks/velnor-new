// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Pair, TestResult, binding, seed};
use crate::journal::{
    Journal, StorageInodePolicy, StorageMedium, StorageObserver, StoragePoolLimit, StorageProfile,
    StorageRole, StorageRoleLimit,
};

pub(super) fn profile() -> TestResult<StorageProfile> {
    let mut pools = vec![StoragePoolLimit::new_with_inode_policy(
        "0".repeat(64),
        StorageMedium::Disk,
        StorageInodePolicy::DynamicBtrfs,
        80_000_000_000,
        0,
        0,
        0,
    )?];
    let mut roles = Vec::new();
    for (index, role) in StorageRole::PHYSICAL.into_iter().enumerate() {
        let medium = role.contract().ok_or("role contract")?.medium;
        let disk = medium == StorageMedium::Disk;
        let pool = if disk {
            "0".repeat(64)
        } else {
            format!("{:064x}", index + 1)
        };
        if !disk {
            pools.push(StoragePoolLimit::new_with_inode_policy(
                pool.clone(),
                medium,
                StorageInodePolicy::Finite,
                64 << 20,
                4096,
                0,
                0,
            )?);
        }
        roles.push(StorageRoleLimit::new(
            role,
            pool,
            medium,
            64 << 20,
            4096,
            if disk { 0 } else { 64 << 20 },
        )?);
    }
    let work = roles
        .iter()
        .find(|r| r.role() == StorageRole::RunnerWork)
        .ok_or("work role")?
        .pool_id()
        .to_owned();
    for role in [StorageRole::CacheLive, StorageRole::CacheStage] {
        roles.push(StorageRoleLimit::new_alias(
            role,
            work.clone(),
            StorageMedium::Tmpfs,
            StorageRole::RunnerWork,
            1 << 20,
            128,
        )?);
    }
    Ok(StorageProfile::new("b".repeat(32), pools, roles)?)
}

pub(super) async fn install(journal: &Journal, pair: Pair) -> TestResult<()> {
    journal
        .configure_capacity_with_operational_limits(
            crate::journal::CapacityLimits {
                jobs: 4,
                cpu_millicores: 100_000,
                memory_bytes: 128 << 30,
            },
            crate::journal::OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 1 << 30,
            },
        )
        .await?;
    let root = super::scalar(
        journal,
        "SELECT docker_root_digest FROM capacity_reservations WHERE id=2",
    )
    .await?;
    let filesystem = crate::launch::resource_probe::filesystem_instance_digest(
        &crate::launch::resource_probe::FilesystemInstanceDigestInput {
            engine_id: "engine-test",
            docker_root_digest: &root,
            guest_boot_id: b"00000000-0000-0000-0000-000000000001",
            mount_source: b"/dev/vda1",
            filesystem_type_name: b"btrfs",
            filesystem_type_magic: 0x9123_683e,
            mount_device_major: 0,
            mount_device_minor: 38,
            statx_device_major: 0,
            statx_device_minor: 38,
            filesystem_id: 77,
        },
    );
    let profile = profile()?;
    let r = pair.reservation;
    let mut sql = profile_rows(&profile, r, &filesystem)?;
    attachments(&mut sql, pair, &filesystem)?;
    let mapping_digest = binding::digest(b"velnor-root-backing-map-v1\0{}");
    sql.push(format!("UPDATE capacity_reservation_root_backing_mappings SET logical_mount_id=90,logical_filesystem_digest='{filesystem}',backing_filesystem_digest='{filesystem}',mapping_digest='{mapping_digest}' WHERE reservation_id={r}"));
    seed::unguarded(journal, &sql).await
}

fn profile_rows(profile: &StorageProfile, r: i64, filesystem: &str) -> TestResult<Vec<String>> {
    let generation = profile.generation_id();
    let digest = profile.profile_digest();
    let mut sql = vec![
        format!(
            "UPDATE host_storage_profiles SET profile_digest='{digest}' WHERE generation_id='{generation}'"
        ),
        format!(
            "UPDATE capacity_reservation_provisioning SET profile_digest='{digest}' WHERE reservation_id={r}"
        ),
        format!("INSERT OR IGNORE INTO host_storage_profile_current VALUES(1,'{generation}')"),
        format!(
            "INSERT INTO capacity_reservation_provisioning_memory VALUES({r},8589934592,4294967296)"
        ),
    ];
    sql.push(format!(
        "INSERT OR IGNORE INTO host_storage_profile_publication VALUES('{generation}','{}')",
        profile
            .controller_publication()
            .ok_or("publication")?
            .digest()
    ));
    for pool in profile.pools() {
        let id = pool.pool_id();
        let disk = pool.medium() == StorageMedium::Disk;
        let medium = if disk { "disk" } else { "tmpfs" };
        let inode = if disk { "dynamic-btrfs" } else { "finite" };
        sql.push(format!("INSERT OR IGNORE INTO host_storage_profile_pools VALUES('{generation}','{id}','{medium}','{inode}',{}, {},0,0)",pool.max_bytes(),pool.max_inodes()));
        let observation = if disk {
            format!("'{filesystem}',100000000000,90000000000,0,0")
        } else {
            "NULL,NULL,NULL,NULL,NULL".to_owned()
        };
        sql.push(format!("INSERT INTO capacity_reservation_provisioning_pools VALUES({r},'{id}','{}','{medium}','{}','{inode}',{},{},0,0,{observation})",if disk{"shared-disk"}else{"exclusive-tmpfs"},if disk{"btrfs"}else{"tmpfs"},pool.max_bytes(),pool.max_inodes()));
    }
    for role in profile.roles() {
        let name = role.role().as_str();
        let pool = role.pool_id();
        let disk = role.medium() == StorageMedium::Disk;
        let medium = if disk { "disk" } else { "tmpfs" };
        let inode = if disk { "dynamic-btrfs" } else { "finite" };
        let kind = if disk { "btrfs" } else { "tmpfs" };
        let retained = role.retained_memory_bytes();
        let bytes = role.max_bytes();
        let inodes = role.max_inodes();
        let alias = role
            .alias_of()
            .map_or("NULL".to_owned(), |a| format!("'{}'", a.as_str()));
        sql.push(format!("INSERT OR IGNORE INTO host_storage_profile_roles VALUES('{generation}','{name}','{pool}','{medium}',{bytes},{inodes},{retained},{alias})"));
        if role.alias_of().is_some() {
            continue;
        }
        let planned = if disk {
            format!("'{filesystem}'")
        } else {
            "NULL".to_owned()
        };
        sql.push(format!("INSERT INTO capacity_reservation_provisioning_roles VALUES({r},'{name}','{pool}','{medium}','{kind}','{inode}',{planned},{bytes},{inodes},{retained})"));
        let counts = if disk {
            "0,0,0".to_owned()
        } else {
            format!("{inodes},{inodes},{inodes}")
        };
        sql.push(format!("INSERT INTO capacity_reservation_bound_storage_roles VALUES({r},'{name}','{pool}','{filesystem}','{medium}','{kind}',{bytes},{bytes},{bytes},{counts},'{inode}',0,0,{bytes},{inodes},{retained})"));
    }
    Ok(sql)
}

fn attachments(sql: &mut Vec<String>, pair: Pair, filesystem: &str) -> TestResult<()> {
    let r = pair.reservation;
    let mut tmp = 51;
    for role in StorageRole::PHYSICAL {
        let disk = role.contract().ok_or("contract")?.medium == StorageMedium::Disk;
        let device = if disk {
            38
        } else {
            let next = tmp;
            tmp += 1;
            next
        };
        for a in role.attachments() {
            let name = role.as_str();
            let observer = match a.observer {
                StorageObserver::Runner => "runner",
                StorageObserver::Dind => "dind",
                StorageObserver::Probe => "probe",
            };
            let container = if observer == "probe" {
                "NULL".to_owned()
            } else {
                format!("'{}'", pair.container_id(observer))
            };
            let source = a
                .source_target
                .map_or("NULL".to_owned(), |s| format!("'{s}'"));
            let target = a.target;
            let read = i64::from(a.read_only);
            let magic = if disk { 0x9123_683e_u64 } else { 0x0102_1994 };
            let fsid = if disk { 77 } else { u64::try_from(device)? };
            sql.push(format!("INSERT INTO capacity_reservation_bound_storage_attachments(reservation_id,role,observer,container_id,mount_target,source_target,read_only,mount_id,device_major,device_minor,statx_device_major,statx_device_minor,fs_type_magic,filesystem_id,filesystem_instance_digest) VALUES({r},'{name}','{observer}',{container},'{target}',{source},{read},90,0,{device},0,{device},{magic},'{fsid:016x}','{filesystem}')"));
            let sub = a
                .volume_subpath
                .map_or("NULL".to_owned(), |s| format!("'{s}'"));
            let root = a
                .volume_subpath
                .map_or("/docker".to_owned(), |s| format!("/docker/{s}"));
            let inode = if target == "/opt/velnor/job-facts" {
                500
            } else {
                100
            };
            sql.push(format!("INSERT INTO capacity_reservation_bound_storage_paths(reservation_id,role,observer,mount_target,mount_root,volume_subpath,statx_inode) VALUES({r},'{name}','{observer}','{target}','{root}',{sub},{inode})"));
        }
    }
    for role in ["runner", "dind"] {
        let id = pair.container_id(role);
        sql.push(format!("INSERT INTO capacity_reservation_bound_cgroups VALUES({r},'{role}','{id}','/docker/{id}',4294967296,0,100000,100000,512)"));
    }
    Ok(())
}
