// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use sha2::{Digest, Sha256};
use velnor_runner_host::RepositoryConnection;

use super::super::TestResult;
use super::{DIND_ID, DIND_IMAGE_ID, PRIVATE_VOLUME, RUNNER_ID, RUNNER_IMAGE_ID};

const RUNNER_NAME: &str = "v0123456789abcdef0123456789abcdef";
const BOOT_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";

fn hex_sha256(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

pub(super) fn docker_root_digest() -> String {
    let first = Sha256::digest(b"velnor-resource-probe-root-v1\0/var/lib/docker");
    hex_sha256(&first)
}

pub(super) async fn seed_completed_launch(
    journal_path: &Path,
    connection: &RepositoryConnection,
) -> TestResult<velnor_runner_host::Journal> {
    let journal = velnor_runner_host::Journal::open(journal_path).await?;
    let canonical = std::fs::canonicalize(journal_path)?;
    let identity = String::from_utf8(connection.canonical_identity_bytes()?)?;
    let root_digest = docker_root_digest();
    let db = turso::Builder::new_local(
        canonical
            .to_str()
            .ok_or_else(|| std::io::Error::other("journal path utf8"))?,
    )
    .build()
    .await?;
    let conn = db.connect()?;
    conn.execute("BEGIN", ()).await?;
    let mut trigger_rows = conn
        .query(
            "SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' AND name IN ('reservation_network_insert_guard', 'cleanup_observer_witness_insert_guard')",
            (),
        )
        .await?;
    let mut triggers = Vec::new();
    while let Some(row) = trigger_rows.next().await? {
        triggers.push((row.get::<String>(0)?, row.get::<String>(1)?));
    }
    drop(trigger_rows);
    for (name, _) in &triggers {
        conn.execute(&format!("DROP TRIGGER {name}"), ()).await?;
    }
    conn.execute(
        "UPDATE journal_meta SET engine_id = 'engine-1' WHERE singleton = 1",
        (),
    )
    .await?;
    seed_worker_images(&conn, &root_digest).await?;
    conn.execute(
        "INSERT INTO connection_scopes (connection_id, identity) VALUES ('conn-test-1', ?1)",
        [identity.as_str()],
    )
    .await?;
    conn.execute(
        "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES ('conn-test-1', 7, 'ubuntu-26.04-scale-set', 'created', 'engine-1', 'unix:///docker.sock', 'linux/amd64')",
        (),
    )
    .await?;
    conn.execute(
        "INSERT INTO connection_intents (id, connection_id, kind, subject, state, cleanup_proven, docker_id, github_runner_id, dind_id, worker_volume, scale_set_id, runner_request_id, runner_name, docker_engine_id, launch_phase, runner_completed) VALUES (1, 'conn-test-1', 'launch', 'req-11', 'done', 0, ?1, '21', ?2, NULL, 7, 11, ?3, 'engine-1', 'complete', 1)",
        (RUNNER_ID, DIND_ID, RUNNER_NAME),
    )
    .await?;
    conn.execute(
        "INSERT INTO connection_completion_cleanup (intent_id, connection_id, scale_set_id, runner_request_id, runner_id, runner_name, runner_absent) VALUES (1, 'conn-test-1', 7, 11, 21, ?1, 1)",
        [RUNNER_NAME],
    )
    .await?;
    seed_connection_and_reservation(&conn, &root_digest).await?;
    seed_storage_attachments_and_witness(&conn, &root_digest).await?;
    for (_, sql) in &triggers {
        conn.execute(sql, ()).await?;
    }
    conn.execute("COMMIT", ()).await?;
    drop(conn);
    drop(db);
    Ok(journal)
}

async fn seed_worker_images(conn: &turso::Connection, root_digest: &str) -> TestResult<()> {
    for (role, sha_ch, arch_ch, idx_ch, man_ch, cfg_id, iidx_ch, lsha_ch, lidx_ch) in [
        (
            "velnor-runner",
            "a",
            "1",
            "2",
            "3",
            RUNNER_IMAGE_ID,
            "5",
            "6",
            "7",
        ),
        (
            "velnor-dind",
            "a",
            "9",
            "a",
            "b",
            DIND_IMAGE_ID,
            "c",
            "d",
            "e",
        ),
        (
            "velnor-runner",
            "b",
            "7",
            "2",
            "3",
            RUNNER_IMAGE_ID,
            "5",
            "6",
            "7",
        ),
    ] {
        let (sha, arch_sha) = (sha_ch.repeat(40), arch_ch.repeat(64));
        let image_ref = format!("{role}:v{sha}-{arch_sha}");
        let receipt =
            format!(r#"{{"store":"classic","image_id":"{cfg_id}","repo_tags":["{image_ref}"]}}"#);
        conn.execute(
            "INSERT INTO image_imports (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt, state, image_id, image_index_digest, image_index_size, manifest_size, load_archive_sha256, load_archive_index_digest, image_reference, receipt) VALUES ('engine-1', ?1, 'aarch64', ?2, ?3, ?4, 'linux/amd64', ?5, ?6, 1, 'completed', ?7, ?8, 256, 512, ?9, ?10, ?11, ?12)",
            (
                root_digest,
                sha.as_str(),
                arch_sha.as_str(),
                idx_ch.repeat(64).as_str(),
                format!("sha256:{}", man_ch.repeat(64)).as_str(),
                cfg_id,
                cfg_id,
                format!("sha256:{}", iidx_ch.repeat(64)).as_str(),
                lsha_ch.repeat(64).as_str(),
                lidx_ch.repeat(64).as_str(),
                image_ref.as_str(),
                receipt.as_str(),
            ),
        )
        .await?;
    }
    Ok(())
}

async fn seed_connection_and_reservation(
    conn: &turso::Connection,
    root_digest: &str,
) -> TestResult<()> {
    let hex64 = "a".repeat(64);
    let hex32 = "b".repeat(32);
    conn.execute(
        "INSERT INTO connection_request_authorizations (connection_id, scale_set_id, request_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json, acquire_state, acquire_response_json, acquire_response_digest) VALUES ('conn-test-1', 7, 11, 1, 'repository', 'created_empty_exclusive_admin_session_v1', ?1, ?1, '{}', 'acquired', '{}', ?1)",
        [hex64.as_str()],
    )
    .await?;
    conn.execute(
        "INSERT INTO connection_runner_jits (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state, runner_id, runner_name, started_request_id, completed_request_id) VALUES (1, 'conn-test-1', 7, 11, ?1, 'completed', 21, ?1, 11, 11)",
        [RUNNER_NAME],
    )
    .await?;
    conn.execute(
        "INSERT INTO host_storage_profiles (generation_id, profile_digest) VALUES (?1, ?2)",
        (hex32.as_str(), hex64.as_str()),
    )
    .await?;
    conn.execute(
        "INSERT INTO capacity_reservations (id, intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state, remote_cleanup_proven, local_cleanup_proven) VALUES (2, 1, 'conn-test-1', 'assign-1', 'engine-1', ?1, ?2, ?3, 1000, 1073741824, 'uncertain', 1, 0)",
        (root_digest, hex64.as_str(), hex32.as_str()),
    )
    .await?;
    conn.execute(
        "INSERT INTO capacity_reservation_provisioning (reservation_id, operation_id, profile_generation_id, profile_digest, sample_generation_id, observed_at_unix_ms, engine_id, docker_root_digest, retained_memory_bytes) VALUES (2, 1, ?1, ?2, ?1, 1700000000000, 'engine-1', ?3, 0)",
        (hex32.as_str(), hex64.as_str(), root_digest),
    )
    .await?;
    conn.execute(
        "INSERT INTO capacity_reservation_provisioning_topology (reservation_id, topology_version) VALUES (2, 1)",
        (),
    )
    .await?;
    for (role, suffix) in [
        ("shared-run", ""),
        ("runner-home", "-home"),
        ("runner-work", "-work"),
        ("dind-data", "-docker"),
    ] {
        let name = format!("{PRIVATE_VOLUME}{suffix}");
        conn.execute(
            "INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, docker_id, publication_policy_digest, create_state) VALUES (2, ?1, ?2, ?2, NULL, 'created')",
            (role, name.as_str()),
        )
        .await?;
    }
    conn.execute(
        "INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state, runner_container_id, dind_container_id, resources_bound, instance_binding_digest, guest_boot_id, jit_state, runner_id, runner_name, runner_scale_set_id, publication_state, projection_sha256, publication_directory_device, publication_directory_inode, publication_mount_id, delivery_state, storage_cleanup_proven) VALUES (2, 'started', ?1, ?2, 1, ?3, ?4, 'cleaned', 21, ?5, 7, 'published', ?3, 1, 1, 1, 'delivered', 0)",
        (RUNNER_ID, DIND_ID, hex64.as_str(), BOOT_ID, RUNNER_NAME),
    )
    .await?;
    let net_name = format!("velnor-{RUNNER_NAME}-private");
    let net_labels = r#"{"io.velnor.connection-id":"conn-test-1","io.velnor.engine-id":"engine-1","io.velnor.managed":"true","io.velnor.operation-id":"1","io.velnor.reservation-id":"2"}"#;
    conn.execute(
        "INSERT INTO capacity_reservation_networks (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, network_name, labels_json, create_state, network_id, remove_state) VALUES (2, 'conn-test-1', 1, 'engine-1', ?1, ?2, ?3, 'created', ?4, 'not_requested')",
        (root_digest, net_name.as_str(), net_labels, hex64.as_str()),
    )
    .await?;
    Ok(())
}

async fn insert_att(
    conn: &turso::Connection,
    who: (&str, &str, Option<&str>),
    paths: (&str, Option<&str>),
    dev: (i64, i64, i64, i64),
) -> TestResult<()> {
    let hex64 = "a".repeat(64);
    conn.execute(
        "INSERT INTO capacity_reservation_bound_storage_attachments (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, filesystem_id, filesystem_instance_digest) VALUES (2, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?9, 2435016766, '0000000000000001', ?10)",
        (who.0, who.1, who.2, paths.0, paths.1, dev.0, dev.1, dev.2, dev.3, hex64.as_str()),
    )
    .await?;
    Ok(())
}

async fn seed_storage_attachments_and_witness(
    conn: &turso::Connection,
    root_digest: &str,
) -> TestResult<()> {
    for (role, target, mid, min) in [
        ("shared-run", "/run", 1_i64, 101_i64),
        ("runner-home", "/home/runner", 2, 102),
        ("runner-work", "/home/runner/_work", 3, 103),
        ("runner-tmp", "/tmp", 4, 104),
        ("runner-shm", "/dev/shm", 5, 105),
    ] {
        insert_att(
            conn,
            (role, "runner", Some(RUNNER_ID)),
            (target, None),
            (0, mid, 0, min),
        )
        .await?;
    }
    for (role, target, mid, min) in [
        ("dind-data", "/var/lib/docker", 6_i64, 106_i64),
        ("dind-tmp", "/tmp", 7, 107),
        ("dind-shm", "/dev/shm", 8, 108),
        ("shared-run", "/run", 9, 101),
        ("runner-home", "/home/runner", 10, 102),
        ("runner-work", "/home/runner/_work", 11, 103),
    ] {
        insert_att(
            conn,
            (role, "dind", Some(DIND_ID)),
            (target, None),
            (0, mid, 0, min),
        )
        .await?;
    }
    insert_att(
        conn,
        ("runner-root", "runner", Some(RUNNER_ID)),
        ("/", None),
        (1, 12, 8, 1),
    )
    .await?;
    insert_att(
        conn,
        ("dind-root", "dind", Some(DIND_ID)),
        ("/", None),
        (1, 13, 8, 1),
    )
    .await?;
    insert_att(
        conn,
        ("engine-metadata", "probe", None),
        ("/velnor/docker-root", Some("/var/lib/docker")),
        (1, 14, 8, 1),
    )
    .await?;
    for (role, cid, path) in [
        ("runner", RUNNER_ID, "/sys/fs/cgroup/docker/runner"),
        ("dind", DIND_ID, "/sys/fs/cgroup/docker/dind"),
    ] {
        conn.execute(
            "INSERT INTO capacity_reservation_bound_cgroups (reservation_id, role, container_id, path, memory_max_bytes, memory_current_bytes, cpu_quota_micros, cpu_period_micros, pids_max) VALUES (2, ?1, ?2, ?3, 1073741824, 1024, 100000, 100000, 512)",
            (role, cid, path),
        )
        .await?;
    }
    let hex64 = "a".repeat(64);
    seed_cleanup_witness(conn, root_digest, &hex64).await
}

async fn seed_cleanup_witness(
    conn: &turso::Connection,
    root_digest: &str,
    hex64: &str,
) -> TestResult<()> {
    let binding_json = format!(
        concat!(
            r#"{{"expected_guest_boot_id":"{boot_id}","#,
            r#""docker_root":{{"target":"/velnor/docker-root","mount_id":14,"statx_mount_id":14,"mount_root":"/var/lib/docker","mount_device":[8,1],"statx_device":[8,1],"filesystem_type_magic":2435016766,"filesystem_id":1,"read_only":true}},"#,
            r#""daemon":{{"pid":100,"start_ticks":10,"namespace_device":[0,3],"namespace_inode":1000}},"#,
            r#""runner":{{"pid":200,"start_ticks":20,"namespace_device":[0,3],"namespace_inode":2000}},"#,
            r#""dind":{{"pid":300,"start_ticks":30,"namespace_device":[0,3],"namespace_inode":3000}},"#,
            r#""cgroup_paths":["/sys/fs/cgroup/docker/runner","/sys/fs/cgroup/docker/dind"],"#,
            r#""volume_mountpoints":["#,
            r#"{{"role":"shared-run","mountpoint":"/var/lib/docker/volumes/v1/_data","mount_device":[0,101],"statx_device":[0,101],"inode":1}},"#,
            r#"{{"role":"runner-home","mountpoint":"/var/lib/docker/volumes/v2/_data","mount_device":[0,102],"statx_device":[0,102],"inode":2}},"#,
            r#"{{"role":"runner-work","mountpoint":"/var/lib/docker/volumes/v3/_data","mount_device":[0,103],"statx_device":[0,103],"inode":3}},"#,
            r#"{{"role":"dind-data","mountpoint":"/var/lib/docker/volumes/v4/_data","mount_device":[0,106],"statx_device":[0,106],"inode":4}}"#,
            r#"],"owned_tmpfs_devices":[[0,101],[0,102],[0,103],[0,104],[0,105],[0,106],[0,107],[0,108]]}}"#
        ),
        boot_id = BOOT_ID,
    );
    let binding_sha256 = hex_sha256(binding_json.as_bytes());
    conn.execute(
        "INSERT INTO capacity_reservation_cleanup_observer_witnesses (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, runner_container_id, dind_container_id, instance_binding_digest, guest_boot_id, binding_json, binding_sha256) VALUES (2, 'conn-test-1', 1, 'engine-1', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        (
            root_digest,
            RUNNER_ID,
            DIND_ID,
            hex64,
            BOOT_ID,
            binding_json.as_str(),
            binding_sha256.as_str(),
        ),
    )
    .await?;
    Ok(())
}
