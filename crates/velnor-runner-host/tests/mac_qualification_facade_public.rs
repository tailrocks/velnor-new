// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[expect(
    dead_code,
    unused_imports,
    reason = "The external Docker peer supports fault scenarios shared by these tests."
)]
#[path = "mac_qualification_facade_fixture.rs"]
mod fixture;

use fixture::{ApiFault, ApiPlan, DockerApiFixture, Probe, RegistryImage};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    error::Error,
    sync::{Arc, Mutex},
};
use velnor_runner_host::{
    HostError,
    resource_probe_release::{
        MacRunnerQualificationRequest, MacRunnerQualifier, RegistryRequest, RegistryResponse,
        RegistryTransport,
    },
};

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LOCK: &str = "runner_version = \"2.338.0\"\ndocker_version = \"29.9.1\"\nbuildx_version = \"0.38.0\"\ncompose_version = \"5.6.0\"\nnode_version = \"26.12.0\"\n";
type TestResult = Result<(), Box<dyn Error>>;

fn hash(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}
fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hash(bytes))
}

struct Registry {
    responses: Mutex<VecDeque<RegistryResponse>>,
    requests: Mutex<Vec<RegistryRequest>>,
}
impl RegistryTransport for Registry {
    fn get(
        &self,
        request: RegistryRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<RegistryResponse, HostError>> + Send + '_>,
    > {
        Box::pin(async move {
            self.requests
                .lock()
                .map_err(|_| HostError::Identity)?
                .push(request);
            self.responses
                .lock()
                .map_err(|_| HostError::Identity)?
                .pop_front()
                .ok_or(HostError::Identity)
        })
    }
}
fn response(status: u16, challenge: Option<String>, body: Vec<u8>) -> RegistryResponse {
    RegistryResponse {
        status,
        challenge,
        location: None,
        body,
    }
}
fn image(
    role: &str,
    context: &str,
    names: &[&str],
) -> Result<(Value, RegistryImage, Vec<RegistryResponse>), Box<dyn Error>> {
    let repository = format!("ghcr.io/tailrocks/velnor-{role}");
    let (user, entrypoint) = match role {
        "runner" => ("runner", "/usr/local/bin/velnor-runner-entrypoint"),
        "dind" => ("", "/usr/local/bin/velnor-dind-entrypoint"),
        _ => ("65532:65532", "/velnor/resource-probe"),
    };
    let mut config_value = json!({"Labels":{"org.opencontainers.image.revision":SOURCE,"io.velnor.image.kind":role,"org.opencontainers.image.created":"2026-01-01T00:00:00Z","org.opencontainers.image.description":"fixture","org.opencontainers.image.title":"ubuntu","org.opencontainers.image.version":"26.04"},"User":user,"WorkingDir":if role == "runner" {"/home/runner"} else {"/"},"Entrypoint":[entrypoint],"Env":["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"]});
    if role == "dind" {
        config_value.as_object_mut().ok_or("config")?.remove("User");
        config_value
            .as_object_mut()
            .ok_or("config")?
            .remove("WorkingDir");
    }
    if role == "resource-probe" {
        config_value["Labels"] = json!({"org.opencontainers.image.revision":SOURCE});
    }
    let config = serde_json::to_vec(
        &json!({"os":"linux","architecture":"amd64","rootfs":{"type":"layers","diff_ids":[format!("sha256:{}", "b".repeat(64))]},"config":config_value}),
    )?;
    let manifest = serde_json::to_vec(
        &json!({"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"mediaType":"application/vnd.oci.image.config.v1+json","digest":digest(&config),"size":config.len()},"layers":[{"mediaType":"application/vnd.oci.image.layer.v1.tar+gzip","digest":format!("sha256:{}", "c".repeat(64)),"size":1}]}),
    )?;
    let manifest_digest = digest(&manifest);
    let config_digest = digest(&config);
    let contexts = names
        .iter()
        .map(|name| (*name, "f".repeat(64)))
        .collect::<BTreeMap<_, _>>();
    let entry = json!({"repository":repository,"tag":format!("sha-{SOURCE}"),"registry_digest":manifest_digest,"platform_manifest_digest":manifest_digest,"config_digest":config_digest,"build_source":{"metadata_sha256":"d".repeat(64),"build_type":"https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md","source_repository":"https://github.com/tailrocks/velnor-new","source_commit":SOURCE,"context_path":context,"dockerfile_path":format!("{context}/Dockerfile"),"builder_platform":"linux/amd64","named_contexts":contexts}});
    let challenge = format!(
        "Bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\",scope=\"repository:tailrocks/velnor-{role}:pull\""
    );
    let replies = vec![
        response(401, Some(challenge), vec![]),
        response(200, None, br#"{"token":"anonymous"}"#.to_vec()),
        response(200, None, manifest.clone()),
        response(200, None, config),
    ];
    Ok((
        entry,
        RegistryImage {
            repository,
            manifest_digest,
            config_digest,
            manifest_size: manifest.len(),
            config: config_value,
        },
        replies,
    ))
}

#[expect(
    clippy::too_many_lines,
    reason = "The scenario binds registry, daemon state, and the exact receipt."
)]
async fn scenario(
    containerd: bool,
    bad_registry: bool,
    fault: Option<ApiFault>,
    mismatch: bool,
) -> TestResult {
    let temp = tempfile::tempdir()?;
    std::fs::set_permissions(
        temp.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    let mut images = Vec::new();
    let mut replies = VecDeque::new();
    let mut entries = Vec::new();
    for (role, context, names) in [
        (
            "runner",
            "images/runner/ubuntu-26.04",
            vec![
                "resource_probe",
                "storage_init",
                "job_start_hook",
                "job_started_hook",
            ],
        ),
        ("dind", "images/dind", vec!["resource_probe"]),
        (
            "resource-probe",
            "images/resource-probe",
            vec!["resource_probe"],
        ),
    ] {
        let (entry, image, responses) = image(role, context, &names)?;
        entries.push(entry);
        images.push(image);
        replies.extend(responses);
    }
    if bad_registry {
        replies[2].body.push(b' ');
    }
    let manifest = json!({"schema_version":2,"repository":"tailrocks/velnor-new","source_ref":"refs/heads/main","signer_workflow":".github/workflows/product-release-images.yml","signer_ref":"refs/heads/main","trusted_signing_identity":{"oidc_issuer":"https://token.actions.githubusercontent.com","certificate_identity":"https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main"},"source_commit":SOURCE,"runner_lock_sha256":hash(LOCK.as_bytes()),"workflow_authority_sha":SOURCE,"platform":"linux/amd64","versions":{"runner":"2.338.0","docker":"29.9.1","buildx":"0.38.0","compose":"5.6.0","node":"26.12.0"},"images":{"runner":entries[0],"dind":entries[1],"resource_probe":entries[2]},"resource_probe":{"protocol_version":1,"image_user":"65532:65532","entrypoint":"/velnor/resource-probe","working_directory":"/"}});
    let bytes = serde_json::to_vec(&manifest)?;
    let manifest_path = temp.path().join("manifest.json");
    let lock_path = temp.path().join("runner.lock");
    let receipt = temp.path().join("receipt.json");
    std::fs::write(&manifest_path, &bytes)?;
    std::fs::write(&lock_path, LOCK)?;
    let mut plan = ApiPlan::new(SOURCE);
    plan.registry_images = images.clone();
    plan.containerd = containerd;
    plan.foreign_sentinel = true;
    plan.fault = fault;
    if mismatch {
        plan.version_mismatch = Some(Probe::RunnerNode);
    }
    plan.runner_id = if containerd {
        images[0].manifest_digest.clone()
    } else {
        images[0].config_digest.clone()
    };
    plan.dind_id = if containerd {
        images[1].manifest_digest.clone()
    } else {
        images[1].config_digest.clone()
    };
    plan.probe_id = if containerd {
        images[2].manifest_digest.clone()
    } else {
        images[2].config_digest.clone()
    };
    let peer = DockerApiFixture::new(plan)?;
    let registry = Arc::new(Registry {
        responses: Mutex::new(replies),
        requests: Mutex::new(Vec::new()),
    });
    let request = MacRunnerQualificationRequest {
        manifest: manifest_path,
        runner_lock: lock_path,
        source_commit: SOURCE.to_owned(),
        run_id: 123,
        run_attempt: 2,
        publisher_artifact_id: 456,
        docker_socket: peer.socket().to_string_lossy().into_owned(),
        state_dir: temp.path().canonicalize()?,
        receipt_output: receipt.clone(),
    };
    let result = MacRunnerQualifier::new(registry.clone())
        .qualify(request)
        .await;
    let snapshot = peer.snapshot()?;
    if bad_registry {
        assert!(result.is_err());
        assert!(!receipt.exists());
        assert!(snapshot.created.is_empty());
        return Ok(());
    }
    if fault.is_some() || mismatch {
        assert!(result.is_err());
        assert!(!receipt.exists());
        assert!(snapshot.fault_reached);
        assert!(snapshot.foreign_container_present && snapshot.foreign_volume_present);
        if fault == Some(ApiFault::ReplacedContainerId) {
            assert!(
                snapshot
                    .containers
                    .iter()
                    .any(|container| container.id == "e".repeat(64))
            );
        }
        if mismatch {
            assert_eq!(snapshot.containers.len(), 1);
            assert_eq!(snapshot.volumes.len(), 1);
        }
        return Ok(());
    }
    assert!(
        result.is_ok(),
        "{result:?}; requests={} created={:?}",
        snapshot.api_requests,
        snapshot.paths
    );
    let actual: Value = serde_json::from_slice(&std::fs::read(receipt)?)?;
    let receipt_image = |image: &RegistryImage| json!({"repository":image.repository,"registry_digest":image.manifest_digest,"platform_manifest_digest":image.manifest_digest,"config_digest":image.config_digest,"local_id":if containerd {&image.manifest_digest} else {&image.config_digest}});
    assert_eq!(
        actual,
        json!({"schema":"velnor.runner-image-mac-qualification.v1","source_commit":SOURCE,"run_id":123,"run_attempt":2,"publisher_artifact_id":456,"manifest_sha256":hash(&bytes),"runner_lock_sha256":hash(LOCK.as_bytes()),"platform":"linux/amd64","images":{"runner":receipt_image(&images[0]),"dind":receipt_image(&images[1]),"resource_probe":receipt_image(&images[2])},"outer_daemon":{"id":"outer-daemon-id","root":"/var/lib/docker","store":if containerd {"containerd"} else {"classic"}},"inner_daemon_id":"inner-daemon-id","versions":{"runner":"2.338.0","docker":"29.9.1","buildx":"0.38.0","compose":"5.6.0","node":"26.12.0"},"cleanup_complete":true})
    );
    assert_eq!(actual["manifest_sha256"], hash(&bytes));
    assert_eq!(actual["runner_lock_sha256"], hash(LOCK.as_bytes()));
    assert_eq!(actual["source_commit"], SOURCE);
    assert_eq!(actual["run_id"], 123);
    assert_eq!(actual["run_attempt"], 2);
    assert_eq!(actual["publisher_artifact_id"], 456);
    assert_eq!(actual["cleanup_complete"], true);
    assert_eq!(
        actual["outer_daemon"]["store"],
        if containerd { "containerd" } else { "classic" }
    );
    for (key, image) in [
        ("runner", &images[0]),
        ("dind", &images[1]),
        ("resource_probe", &images[2]),
    ] {
        assert_eq!(
            actual["images"][key]["registry_digest"],
            image.manifest_digest
        );
        assert_eq!(
            actual["images"][key]["local_id"],
            if containerd {
                image.manifest_digest.as_str()
            } else {
                image.config_digest.as_str()
            }
        );
    }
    assert!(snapshot.foreign_container_present && snapshot.foreign_volume_present);
    assert_eq!(snapshot.containers.len(), 1);
    assert_eq!(snapshot.volumes.len(), 1);
    for container in snapshot.created {
        assert_eq!(container.host_config["NetworkMode"], "none");
        for mount in container.host_config["Mounts"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if mount["Type"] == "bind" {
                assert_eq!(container.role, "resource-probe");
                assert_eq!(mount["Source"], "/var/lib/docker");
                assert_eq!(mount["Target"], "/velnor/docker-root");
                assert_eq!(mount["ReadOnly"], true);
            }
        }
        let serialized = container.request.to_string();
        assert!(
            !serialized.contains("/Users/")
                && !serialized.contains("/var/run/docker.sock\":")
                && !serialized.to_ascii_lowercase().contains("token")
        );
    }
    let requests = registry.requests.lock().map_err(|_| "registry lock")?;
    assert_eq!(requests.len(), 12);
    assert!(
        requests
            .iter()
            .all(|request| request.url.starts_with("https://ghcr.io/"))
    );
    Ok(())
}

#[tokio::test]
async fn source_free_facade_qualifies_classic_and_containerd() -> TestResult {
    scenario(false, false, None, false).await?;
    scenario(true, false, None, false).await
}
#[tokio::test]
async fn corrupted_registry_graph_never_emits_receipt() -> TestResult {
    scenario(false, true, None, false).await
}

#[tokio::test]
async fn failed_version_or_cleanup_never_emits_receipt() -> TestResult {
    scenario(false, false, None, true).await?;
    scenario(false, false, Some(ApiFault::CleanupServerError), false).await
}

#[tokio::test]
async fn unsafe_probe_mounts_and_privileges_never_emit_receipt() -> TestResult {
    for fault in [
        ApiFault::ProbeCredential,
        ApiFault::ProbeWrongSource,
        ApiFault::ProbeWrongTarget,
        ApiFault::ProbeWritable,
        ApiFault::ProbeExtraBind,
        ApiFault::ProbeExtraCapability,
        ApiFault::ProbeExtraDevice,
    ] {
        scenario(false, false, Some(fault), false).await?;
    }
    Ok(())
}

#[tokio::test]
async fn replaced_container_is_preserved_and_never_emits_receipt() -> TestResult {
    scenario(false, false, Some(ApiFault::ReplacedContainerId), false).await
}
