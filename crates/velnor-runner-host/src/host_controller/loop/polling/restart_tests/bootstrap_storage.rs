// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::TestResult;
use crate::journal::{
    StorageInodePolicy, StorageMedium, StoragePoolLimit, StorageProfile, StorageRole,
    StorageRoleLimit,
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
pub(super) fn profile_rows(
    profile: &StorageProfile,
    r: i64,
    filesystem: &str,
) -> TestResult<Vec<String>> {
    let generation = profile.generation_id();
    let digest = profile.profile_digest();
    let publication = profile
        .controller_publication()
        .ok_or("publication")?
        .digest();
    let mut sql = vec![
        format!(
            "UPDATE host_storage_profiles SET profile_digest='{digest}' WHERE generation_id='{generation}'"
        ),
        format!(
            "UPDATE capacity_reservation_provisioning SET profile_digest='{digest}' WHERE reservation_id={r}"
        ),
        format!(
            "UPDATE capacity_reservation_provisioning_objects SET publication_policy_digest='{publication}' WHERE reservation_id={r} AND role='engine-metadata'"
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
    }
    Ok(sql)
}
