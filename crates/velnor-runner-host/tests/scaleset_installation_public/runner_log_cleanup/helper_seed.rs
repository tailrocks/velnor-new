// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::super::super::TestResult;
use super::seed::docker_root_digest;
use super::{DIND_ID, PRIVATE_VOLUME, RUNNER_ID};

pub(super) const PUBLICATION_OPERATION: &str = "c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0";
pub(super) const PUBLICATION_ID: &str =
    "6666666666666666666666666666666666666666666666666666666666666666";
pub(super) const INITIALIZER_ID: &str =
    "5555555555555555555555555555555555555555555555555555555555555555";
pub(super) const INITIALIZER_NAME: &str = "velnor-init-1-1";
pub(super) const INITIALIZER_KEY: &str = "1-1";
const RUNNER_NAME: &str = "v0123456789abcdef0123456789abcdef";
const BOOT_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";
const SOURCE_REVISION: &str = "7777777777777777777777777777777777777777";
const RUNTIME_IMAGE: &str =
    "sha256:9999999999999999999999999999999999999999999999999999999999999999";
const HEX64_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEX64_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TABLES: &[&str] = &[
    "resource_probe_operations",
    "host_operation_leases",
    "execution_publication_operations",
    "capacity_reservation_storage_initialization",
    "capacity_reservation_storage_handoffs",
];

pub(super) fn metadata_volume() -> String {
    format!("{PRIVATE_VOLUME}-metadata")
}

pub(super) fn publication_name() -> String {
    format!("velnor-execution-publication-{PUBLICATION_OPERATION}")
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)],
                DIGITS[usize::from(byte & 0x0f)],
            ]
        })
        .map(char::from)
        .collect()
}

fn sha256(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", Value::String(key.clone()), canonical(&map[key])))
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

fn labels(instance_id: &str) -> Value {
    json!({
        "velnor.role": "execution-publication",
        "velnor.operation": PUBLICATION_OPERATION,
        "velnor.instance": instance_id,
        "velnor.engine": sha256("engine-1"),
        "velnor.probe.binding": HEX64_B,
        "org.opencontainers.image.revision": SOURCE_REVISION,
    })
}

pub(super) fn publication_engine_label() -> String {
    sha256("engine-1")
}

pub(super) fn publication_labels(instance_id: &str, engine: &str) -> Value {
    let mut value = labels(instance_id);
    value["velnor.engine"] = json!(engine);
    value
}

pub(super) fn publication_inspect(instance_id: &str, engine: &str) -> Value {
    let volume = metadata_volume();
    json!({
        "Id": PUBLICATION_ID,
        "Name": format!("/{}", publication_name()),
        "Image": RUNTIME_IMAGE,
        "Path": "/velnor/resource-probe",
        "Args": ["--mode", "execution_publication"],
        "Config": {"Image": RUNTIME_IMAGE, "Labels": publication_labels(instance_id, engine), "NetworkDisabled": true},
        "HostConfig": {
            "Privileged": false, "ReadonlyRootfs": true, "NetworkMode": "none",
            "CapDrop": ["ALL"], "CapAdd": ["DAC_OVERRIDE"], "AutoRemove": false,
            "SecurityOpt": ["no-new-privileges:true"], "CgroupnsMode": "private",
            "CpuPeriod": 100_000, "CpuQuota": 10_000, "Memory": 134_217_728_i64,
            "MemorySwap": 134_217_728_i64, "PidsLimit": 4,
            "Mounts": [{"Type": "volume", "Source": volume, "Target": "/opt/velnor", "ReadOnly": false, "VolumeOptions": {"NoCopy": true}}]
        },
        "Mounts": [{"Type": "volume", "Name": volume, "Driver": "local", "Source": format!("/var/lib/docker/volumes/{volume}/_data"), "Destination": "/opt/velnor", "RW": true}],
        "State": {"Running": false, "Status": "created"}
    })
}

pub(super) fn initializer_inspect(volumes: &[String]) -> Value {
    let mounts: Vec<Value> = volumes
        .iter()
        .map(|name| json!({"Type": "volume", "Name": name, "Destination": "/opt/velnor", "RW": true}))
        .collect();
    json!({
        "Id": INITIALIZER_ID,
        "Name": format!("/{INITIALIZER_NAME}"),
        "Config": {"Labels": {"velnor.role": "storage-initializer", "velnor.operation": INITIALIZER_KEY}},
        "Mounts": mounts,
        "State": {"Running": false, "Status": "created"}
    })
}

fn recovery_json(instance_id: &str, root_digest: &str) -> (String, String, String) {
    let name = publication_name();
    let volume = metadata_volume();
    let config = json!({"Image": RUNTIME_IMAGE});
    let options = json!({"name": name, "platform": "linux/amd64"});
    let projection_digest = sha256(&canonical(&json!([options, config])));
    let expected =
        json!({"Image": RUNTIME_IMAGE, "Labels": labels(instance_id), "NetworkDisabled": true});
    let directory = json!({
        "target": "/opt/velnor/source-proof/control", "inode": 1, "owner_uid": 0, "owner_gid": 0,
        "mode": 0o555, "mount_id": 5, "statx_mount_id": 5, "mount_root": "/ctl",
        "mount_source": "/dev/vda1", "mount_device_major": 254, "mount_device_minor": 1,
        "statx_device_major": 254, "statx_device_minor": 1, "filesystem_type_magic": 1,
        "filesystem_type_name": "ext4", "filesystem_id": 1, "read_only": true, "executable_mount": false
    });
    let value = json!({
        "schema_version": 2, "operation_id": PUBLICATION_OPERATION, "instance_id": instance_id,
        "engine_id": "engine-1", "docker_root_digest": root_digest, "operation_name": name,
        "platform": "linux/amd64", "source_revision": SOURCE_REVISION,
        "runtime_image_id": RUNTIME_IMAGE, "image_binding_digest": HEX64_B,
        "projection_digest": projection_digest, "metadata_volume_name": volume,
        "metadata_volume_id": volume,
        "metadata_volume_mountpoint": format!("/var/lib/docker/volumes/{volume}/_data"),
        "expected_payload_sha256": HEX64_A, "expected_guest_image_sha256": null,
        "expected_runtime_identity": null, "parent_kind": "challenge", "parent_operation_id": 1,
        "reservation_id": 2, "connection_id": "conn-test-1", "runner_id": 21,
        "runner_container_id": RUNNER_ID, "dind_container_id": DIND_ID,
        "metadata_layout_version": 2, "metadata_layout_digest": HEX64_A, "guest_boot_id": BOOT_ID,
        "publisher_mode": "execution_publication", "runner_directory": directory,
        "config": config, "expected_config": expected
    });
    let text = canonical(&value);
    let digest = sha256(&text);
    (text, digest, projection_digest)
}

pub(super) async fn seed_helper_rows(
    journal_path: &Path,
    publication: bool,
    initializer: bool,
    publication_removed: bool,
) -> TestResult<String> {
    let canonical_path = std::fs::canonicalize(journal_path)?;
    let db = turso::Builder::new_local(canonical_path.to_str().ok_or("journal path utf8")?)
        .build()
        .await?;
    let conn = db.connect()?;
    let mut rows = conn
        .query(
            "SELECT instance_id FROM journal_meta WHERE singleton = 1",
            (),
        )
        .await?;
    let instance_id: String = rows.next().await?.ok_or("instance id")?.get(0)?;
    drop(rows);
    conn.execute("BEGIN", ()).await?;
    let triggers = drop_triggers(&conn).await?;
    if publication {
        seed_publication(&conn, &instance_id).await?;
        if publication_removed {
            conn.execute("UPDATE resource_probe_operations SET phase = 'remove_requested' WHERE operation_id = ?1", [PUBLICATION_OPERATION]).await?;
        }
    }
    if initializer {
        seed_initializer(&conn).await?;
    }
    for sql in triggers {
        conn.execute(&sql, ()).await?;
    }
    conn.execute("COMMIT", ()).await?;
    Ok(instance_id)
}

async fn drop_triggers(conn: &turso::Connection) -> TestResult<Vec<String>> {
    let mut found = Vec::new();
    for table in TABLES.iter().copied() {
        let mut rows = conn
            .query(
                "SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = ?1",
                [table],
            )
            .await?;
        while let Some(row) = rows.next().await? {
            found.push((row.get::<String>(0)?, row.get::<String>(1)?));
        }
    }
    for (name, _) in &found {
        conn.execute(&format!("DROP TRIGGER {name}"), ()).await?;
    }
    Ok(found.into_iter().map(|(_, sql)| sql).collect())
}

async fn seed_publication(conn: &turso::Connection, instance_id: &str) -> TestResult<()> {
    let root = docker_root_digest();
    let name = publication_name();
    let volume = metadata_volume();
    let (json_text, json_sha, projection_digest) = recovery_json(instance_id, &root);
    conn.execute(
        "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, container_id, helper_role, probe_purpose) VALUES (?1, ?2, 'engine-1', ?3, ?4, ?5, ?6, ?7, 'resource-probe', ?8, 'container_created', ?9, 'execution-publication', 'none')",
        (PUBLICATION_OPERATION, instance_id, root.as_str(), SOURCE_REVISION, RUNTIME_IMAGE, HEX64_B, name.as_str(), projection_digest.as_str(), PUBLICATION_ID),
    )
    .await?;
    conn.execute(
        "INSERT INTO host_operation_leases (singleton, operation_id, engine_id, docker_root_digest, operation_name, container_id, cpu_millicores, memory_bytes, state) VALUES (1, ?1, 'engine-1', ?2, ?3, ?4, 100, 134217728, 'active')",
        (PUBLICATION_OPERATION, root.as_str(), name.as_str(), PUBLICATION_ID),
    )
    .await?;
    conn.execute(
        "INSERT INTO execution_publication_operations (operation_id, parent_kind, parent_operation_id, reservation_id, connection_id, scale_set_id, witness_request_id, actual_request_id, runner_id, runner_name, runner_container_id, dind_container_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, metadata_layout_version, metadata_layout_digest, guest_boot_id, payload_sha256, claim_digest, projection_digest, context_digest, recovery_projection_json, recovery_projection_sha256) VALUES (?1, 'challenge', 1, 2, 'conn-test-1', 7, 11, NULL, 21, ?2, ?3, ?4, 'engine-1', ?5, ?6, ?6, 2, ?7, ?8, ?7, ?7, ?9, ?7, ?10, ?11)",
        (PUBLICATION_OPERATION, RUNNER_NAME, RUNNER_ID, DIND_ID, root.as_str(), volume.as_str(), HEX64_A, BOOT_ID, projection_digest.as_str(), json_text.as_str(), json_sha.as_str()),
    )
    .await?;
    Ok(())
}

async fn seed_initializer(conn: &turso::Connection) -> TestResult<()> {
    let root = docker_root_digest();
    conn.execute(
        "INSERT INTO capacity_reservation_storage_initialization (reservation_id, connection_id, operation_id, generation, engine_id, docker_root_digest, image_runtime_id, image_source_sha, nano_cpus, memory_bytes, pids_limit, home_volume, work_volume, layout_version, metadata_volume, metadata_volume_id, metadata_layout_digest, helper_name, helper_id, create_dispatch_state, create_state, start_state, result_state, remove_state, abort_state, operation_key, home_device_major, home_device_minor, home_inode, home_mount_id, work_device_major, work_device_minor, work_inode, work_mount_id) VALUES (2, 'conn-test-1', 1, 1, 'engine-1', ?1, ?2, ?3, 1000000000, 134217728, 4, ?4, ?5, 2, ?6, ?6, ?7, ?8, ?9, 'response_received', 'created', 'started', 'confirmed', 'not_requested', 'not_requested', ?10, 0, 1, 1, 1, 0, 2, 2, 2)",
        (
            root.as_str(),
            RUNTIME_IMAGE,
            SOURCE_REVISION,
            format!("{PRIVATE_VOLUME}-home"),
            format!("{PRIVATE_VOLUME}-work"),
            metadata_volume(),
            HEX64_A,
            INITIALIZER_NAME,
            INITIALIZER_ID,
            INITIALIZER_KEY,
        ),
    )
    .await?;
    conn.execute(
        "INSERT INTO capacity_reservation_storage_handoffs (reservation_id, connection_id, operation_id, generation, helper_name, helper_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, operation_key, layout_digest, ready_digest, state, mount_evidence_digest, runner_container_id, dind_container_id) VALUES (2, 'conn-test-1', 1, 1, ?1, ?2, 'engine-1', ?3, ?4, ?4, ?5, ?6, ?7, 'attached', ?7, ?8, ?9)",
        (
            INITIALIZER_NAME,
            INITIALIZER_ID,
            root.as_str(),
            metadata_volume(),
            INITIALIZER_KEY,
            HEX64_A,
            HEX64_B,
            RUNNER_ID,
            DIND_ID,
        ),
    )
    .await?;
    Ok(())
}
