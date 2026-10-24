use super::*;

fn request(operation_id: &str) -> velnor_resource_probe::worker_binding::OwnedCgroupRequest {
    velnor_resource_probe::worker_binding::OwnedCgroupRequest {
        schema_version: 2,
        operation_id: operation_id.to_owned(),
        runner_container_id: "a".repeat(64),
        runner_init_pid: 101,
        runner_init_start_ticks: 1001,
        runner_helper_pid: 102,
        runner_helper_start_ticks: 1002,
        dind_container_id: "b".repeat(64),
        dind_init_pid: 201,
        dind_init_start_ticks: 2001,
        dind_helper_pid: 202,
        dind_helper_start_ticks: 2002,
        docker_root_path: "/var/lib/docker".to_owned(),
    }
}

fn digest(request_json: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"velnor-owned-cgroups-request-v1\0");
    hasher.update(request_json.as_bytes());
    hasher.update(b"\n");
    encode_hex(&hasher.finalize())
}

#[test]
fn owned_cgroup_request_is_bound_to_canonical_bytes_and_operation() {
    let operation_id = "0123456789abcdef0123456789abcdef";
    let request_json = serde_json::to_string(&request(operation_id)).unwrap_or_default();
    let request_sha256 = digest(&request_json);
    let seed = ProbeSeed {
        operation_id: operation_id.to_owned(),
        instance_id: "1".repeat(32),
        engine_id: "selected-engine".to_owned(),
        docker_root_digest: "2".repeat(64),
        source_revision: "3".repeat(40),
        runtime_image_id: format!("sha256:{}", "4".repeat(64)),
        image_binding_digest: "5".repeat(64),
        operation_name: format!("velnor-resource-probe-{operation_id}"),
        projection_digest: "6".repeat(64),
        purpose: ResourceProbePurpose::OwnedCgroups,
        request_sha256: request_sha256.clone(),
    };

    assert!(owned_request_valid(&seed, &request_json));
    assert!(!owned_request_valid(&seed, &format!("{request_json} ")));
    assert!(!owned_request_valid(
        &seed,
        &request_json.replace(operation_id, &"f".repeat(32))
    ));

    let mut wrong_hash = seed;
    wrong_hash.request_sha256 = "7".repeat(64);
    assert!(!owned_request_valid(&wrong_hash, &request_json));
}

#[test]
fn recovered_owned_context_must_match_its_durable_digest() {
    let operation_id = "0123456789abcdef0123456789abcdef";
    let request_json = serde_json::to_string(&request(operation_id)).unwrap_or_default();
    let request_sha256 = digest(&request_json);
    let work = decode_work(
        operation_id,
        HostHelperRole::ResourceProbe,
        "owned-cgroups",
        Some(request_sha256.clone()),
        Some(request_json.clone()),
    );
    assert!(work.is_ok());
    assert!(
        decode_work(
            operation_id,
            HostHelperRole::ResourceProbe,
            "owned-cgroups",
            Some("8".repeat(64)),
            Some(request_json),
        )
        .is_err()
    );
    assert!(
        decode_work(
            operation_id,
            HostHelperRole::ResourceProbe,
            "capacity",
            Some(request_sha256),
            None,
        )
        .is_err()
    );
}
