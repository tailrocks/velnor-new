use serde_json::{Value, json};

pub(super) fn publication_output(line: &[u8]) -> Result<Vec<u8>, String> {
    let request: Value = serde_json::from_slice(line).map_err(|error| error.to_string())?;
    let payload_hex = request["payload_hex"]
        .as_str()
        .ok_or_else(|| "The publisher request has no payload bytes.".to_owned())?;
    let payload_sha256 = request["payload_sha256"]
        .as_str()
        .ok_or_else(|| "The publisher request has no payload digest.".to_owned())?;
    let payload_bytes = u64::try_from(payload_hex.len() / 2).map_err(|error| error.to_string())?;
    let operation_id = request["operation_id"]
        .as_str()
        .ok_or_else(|| "The publisher request has no operation ID.".to_owned())?;
    let boot_id = request["expected_guest_boot_id"]
        .as_str()
        .ok_or_else(|| "The publisher request has no boot ID.".to_owned())?;
    let record = if let Some(directory) = request.get("expected_runner_control") {
        control_publication_record(
            directory,
            &request,
            operation_id,
            boot_id,
            payload_sha256,
            payload_bytes,
        )?
    } else if let Some(directory) = request.get("expected_runner_job_facts") {
        job_facts_publication_record(
            directory,
            &request["expected_runner_runtime_identity"],
            operation_id,
            boot_id,
            payload_sha256,
            request["job_facts_sha256"].as_str().unwrap_or_default(),
            payload_bytes,
            request["guest_image_sha256"].as_str().unwrap_or_default(),
            request["publication_binding_sha256"]
                .as_str()
                .unwrap_or_default(),
            u64::try_from(
                request["guest_image_hex"]
                    .as_str()
                    .unwrap_or_default()
                    .len()
                    / 2,
            )
            .map_err(|error| error.to_string())?,
        )?
    } else {
        return Err("The publisher request has no measured target directory.".to_owned());
    };
    let mut output = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    output.push(b'\n');
    Ok(output)
}

fn control_publication_record(
    directory: &Value,
    request: &Value,
    operation_id: &str,
    boot_id: &str,
    digest: &str,
    payload_bytes: u64,
) -> Result<Value, String> {
    let mount_root = directory["mount_root"]
        .as_str()
        .and_then(|value| value.strip_suffix("/source-proof/control"))
        .ok_or_else(|| "Control directory mount root is invalid".to_owned())?;
    let kind = request["kind"]
        .as_str()
        .ok_or_else(|| "Publisher control kind is absent".to_owned())?;
    let file_name = match kind {
        "challenge" => "challenge.json",
        "decision" => "decision.json",
        _ => return Err("Publisher control kind is invalid".to_owned()),
    };
    let record = json!({
        "schema_version": 1,
        "operation_id": operation_id,
        "guest_boot_id": boot_id,
        "mount_namespace_id": 7002,
        "kind": kind,
        "payload_sha256": digest,
        "file": {
            "path": format!("/opt/velnor/source-proof/control/{file_name}"),
            "control_directory_inode": directory["inode"],
            "inode": 1067,
            "bytes": payload_bytes,
            "owner_uid": 0,
            "owner_gid": 0,
            "mode": 0o444,
            "mount_id": 1508,
            "statx_mount_id": 1508,
            "mount_device_major": directory["mount_device_major"],
            "mount_device_minor": directory["mount_device_minor"],
            "statx_device_major": directory["statx_device_major"],
            "statx_device_minor": directory["statx_device_minor"],
            "filesystem_type_magic": directory["filesystem_type_magic"],
            "filesystem_id": directory["filesystem_id"],
            "mount_root": mount_root,
            "mount_source": directory["mount_source"],
            "readback_sha256": digest
        }
    });
    Ok(record)
}

fn job_facts_publication_record(
    directory: &Value,
    runtime_identity: &Value,
    operation_id: &str,
    boot_id: &str,
    digest: &str,
    facts_digest: &str,
    payload_bytes: u64,
    guest_image_digest: &str,
    binding_digest: &str,
    guest_image_bytes: u64,
) -> Result<Value, String> {
    if directory["inode"].as_u64().unwrap_or_default() == 0 {
        return Err("Job-facts directory inode is absent".to_owned());
    }
    let mut observed = directory.clone();
    observed["read_only"] = json!(false);
    observed["mount_root"] = json!(
        directory["mount_root"]
            .as_str()
            .and_then(|root| root.strip_suffix("/job-facts"))
            .ok_or_else(|| "Job-facts mount root is invalid".to_owned())?
    );
    let mut runtime_observed = directory.clone();
    runtime_observed["target"] = json!("/opt/velnor/runtime-identity");
    runtime_observed["inode"] = runtime_identity["inode"].clone();
    runtime_observed["owner_uid"] = runtime_identity["owner_uid"].clone();
    runtime_observed["owner_gid"] = runtime_identity["owner_gid"].clone();
    runtime_observed["mode"] = runtime_identity["mode"].clone();
    runtime_observed["mount_root"] = observed["mount_root"].clone();
    runtime_observed["read_only"] = json!(false);
    let record = json!({
        "schema_version": 2,
        "operation_id": operation_id,
        "guest_boot_id": boot_id,
        "mount_namespace_id": 7002,
        "payload_sha256": digest,
        "guest_image_sha256": guest_image_digest,
        "publication_binding_sha256": binding_digest,
        "directory": observed,
        "runtime_identity_directory": runtime_observed,
        "file": {
            "path": "/opt/velnor/job-facts/storage-cache-admission.json",
            "inode": 1065,
            "bytes": payload_bytes,
            "owner_uid": 0,
            "owner_gid": 0,
            "mode": 0o444,
            "readback_sha256": facts_digest
        },
        "guest_image_file": {
            "path": "/opt/velnor/runtime-identity/guest-image.json",
            "inode": 1066,
            "bytes": guest_image_bytes,
            "owner_uid": 0,
            "owner_gid": 0,
            "mode": 0o444,
            "readback_sha256": guest_image_digest
        }
    });
    Ok(record)
}
