// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "runner_log_cleanup.rs"]
mod runner_log_cleanup;

use serde_json::{Value, json};
use tar::{Builder, EntryType, Header};
use velnor_runner_host::{HostError, WorkerImage, verify_worker_attestation_manifest};

use self::runner_log_cleanup::run_runner_log_cleanup_scenario;
use super::{MANIFEST_DIGEST, RUNNER_NAMED_LOCALS, SOURCE_SHA, TestResult, slsa_statement};

const LAYER_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const CONFIG_DIGEST: &str =
    "sha256:5555555555555555555555555555555555555555555555555555555555555555";
const ATTESTATION_DIGEST: &str =
    "sha256:6666666666666666666666666666666666666666666666666666666666666666";

fn attestation_descriptor(artifact_type: Option<&str>) -> Value {
    let mut descriptor = json!({
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "digest": ATTESTATION_DIGEST,
        "size": 512,
        "platform": {
            "architecture": "unknown",
            "os": "unknown"
        },
        "annotations": {
            "vnd.docker.reference.digest": MANIFEST_DIGEST,
            "vnd.docker.reference.type": "attestation-manifest"
        }
    });
    if let (Some(map), Some(kind)) = (descriptor.as_object_mut(), artifact_type) {
        map.insert("artifactType".to_owned(), json!(kind));
    }
    descriptor
}

fn legacy_attestation_manifest() -> (Value, Value) {
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": CONFIG_DIGEST,
            "size": 128
        },
        "layers": [{
            "mediaType": "application/vnd.in-toto+json",
            "digest": LAYER_DIGEST,
            "size": 256,
            "annotations": {
                "in-toto.io/predicate-type": "https://slsa.dev/provenance/v1"
            }
        }]
    });
    let config = json!({
        "architecture": "unknown",
        "os": "unknown",
        "config": {},
        "rootfs": {
            "type": "layers",
            "diff_ids": [LAYER_DIGEST]
        }
    });
    (manifest, config)
}

fn oci_artifact_attestation_manifest(subject_digest: &str, artifact_type: &str) -> (Value, Value) {
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "artifactType": artifact_type,
        "config": {
            "mediaType": "application/vnd.oci.empty.v1+json",
            "digest": CONFIG_DIGEST,
            "size": 2
        },
        "layers": [{
            "mediaType": "application/vnd.in-toto+json",
            "digest": LAYER_DIGEST,
            "size": 256,
            "annotations": {
                "in-toto.io/predicate-type": "https://slsa.dev/provenance/v1"
            }
        }],
        "subject": {
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": subject_digest,
            "size": 1024
        }
    });
    (manifest, json!({}))
}

#[test]
fn public_verify_worker_attestation_accepts_legacy_and_oci_artifact_manifests_and_rejects_mismatches()
-> TestResult<()> {
    let statement = slsa_statement(WorkerImage::Runner, &RUNNER_NAMED_LOCALS, SOURCE_SHA);
    let legacy_desc = attestation_descriptor(None);
    let (legacy_manifest, legacy_config) = legacy_attestation_manifest();
    verify_worker_attestation_manifest(
        WorkerImage::Runner,
        SOURCE_SHA,
        MANIFEST_DIGEST,
        1024,
        &legacy_desc,
        &legacy_manifest,
        &legacy_config,
        &statement,
    )?;

    let oci_desc =
        attestation_descriptor(Some("application/vnd.docker.attestation.manifest.v1+json"));
    let (oci_manifest, oci_config) = oci_artifact_attestation_manifest(
        MANIFEST_DIGEST,
        "application/vnd.docker.attestation.manifest.v1+json",
    );
    verify_worker_attestation_manifest(
        WorkerImage::Runner,
        SOURCE_SHA,
        MANIFEST_DIGEST,
        1024,
        &oci_desc,
        &oci_manifest,
        &oci_config,
        &statement,
    )?;

    let (wrong_subject_manifest, _) = oci_artifact_attestation_manifest(
        LAYER_DIGEST,
        "application/vnd.docker.attestation.manifest.v1+json",
    );
    let (wrong_artifact_manifest, _) =
        oci_artifact_attestation_manifest(MANIFEST_DIGEST, "application/vnd.unknown.manifest+json");
    let non_empty_config = json!({"unexpected": 1});
    let mut injected_arg_statement = statement.clone();
    injected_arg_statement
        .pointer_mut("/predicate/buildDefinition/externalParameters/request/args")
        .and_then(Value::as_object_mut)
        .ok_or("args map")?
        .insert("build-arg:RUNNER_VERSION".to_owned(), json!("2.999.0"));
    let mut injected_root_statement = statement.clone();
    injected_root_statement
        .pointer_mut("/predicate/buildDefinition/externalParameters/request")
        .and_then(Value::as_object_mut)
        .ok_or("request map")?
        .insert("root".to_owned(), json!({"request": {}}));
    for (manifest, config, stmt) in [
        (&wrong_subject_manifest, &oci_config, &statement),
        (&wrong_artifact_manifest, &oci_config, &statement),
        (&oci_manifest, &non_empty_config, &statement),
        (&oci_manifest, &oci_config, &injected_arg_statement),
        (&oci_manifest, &oci_config, &injected_root_statement),
    ] {
        assert!(matches!(
            verify_worker_attestation_manifest(
                WorkerImage::Runner,
                SOURCE_SHA,
                MANIFEST_DIGEST,
                1024,
                &oci_desc,
                manifest,
                config,
                stmt
            ),
            Err(HostError::Docker)
        ));
    }
    Ok(())
}

fn runner_log_tar_with_trace() -> TestResult<Vec<u8>> {
    let mut builder = Builder::new(Vec::new());
    let mut dir_header = Header::new_ustar();
    dir_header.set_entry_type(EntryType::Directory);
    dir_header.set_size(0);
    dir_header.set_mode(0o700);
    dir_header.set_uid(1000);
    dir_header.set_gid(1000);
    dir_header.set_mtime(0);
    dir_header.set_cksum();
    builder.append_data(&mut dir_header, ".", std::io::empty())?;

    let log_line = b"[2026-01-01 00:00:01Z INFO Listener] Connected to GitHub\n";
    let mut file_header = Header::new_ustar();
    file_header.set_entry_type(EntryType::Regular);
    file_header.set_size(log_line.len() as u64);
    file_header.set_mode(0o600);
    file_header.set_uid(1000);
    file_header.set_gid(1000);
    file_header.set_mtime(0);
    file_header.set_cksum();
    builder.append_data(
        &mut file_header,
        "Runner_20260101-000001-utc.log",
        &log_line[..],
    )?;
    Ok(builder.into_inner()?)
}

#[test]
fn public_runner_log_capture_completes_as_unavailable_when_diag_missing_or_tampered_and_rejects_bad_exits()
-> TestResult<()> {
    for exit_code in [3_i64, 24_i64] {
        let obs = run_runner_log_cleanup_scenario(exit_code, Vec::new())?;
        obs.first?;
        obs.replay.ok_or("missing replay")??;
        assert!(!obs.runner_present);
        assert_eq!(obs.exec_creates, 1);
        let state = obs.capture_state.ok_or("missing capture state")?;
        assert_eq!(state["phase"], json!("unavailable"));
        assert_eq!(state["exit_code"], json!(exit_code));
        assert_eq!(state["has_trace"], json!(false));
        assert!(obs.runner_logs_projection.is_some());
    }

    let complete_obs = run_runner_log_cleanup_scenario(0, runner_log_tar_with_trace()?)?;
    complete_obs.first?;
    complete_obs.replay.ok_or("missing replay")??;
    assert!(!complete_obs.runner_present);
    assert_eq!(complete_obs.exec_creates, 1);
    let complete_state = complete_obs.capture_state.ok_or("missing capture state")?;
    assert_eq!(complete_state["phase"], json!("complete"));
    assert_eq!(complete_state["exit_code"], json!(0));
    assert_eq!(complete_state["has_trace"], json!(true));
    assert!(complete_obs.runner_logs_projection.is_some());

    for bad_exit in [3_i64, 24_i64] {
        let bad_obs = run_runner_log_cleanup_scenario(bad_exit, b"unexpected".to_vec())?;
        assert!(matches!(bad_obs.first, Err(HostError::Docker)));
        assert!(bad_obs.replay.is_none());
        assert!(bad_obs.runner_present);
    }

    let exit1_obs = run_runner_log_cleanup_scenario(1, Vec::new())?;
    assert!(matches!(exit1_obs.first, Err(HostError::Journal)));
    assert!(exit1_obs.replay.is_none());
    assert!(exit1_obs.runner_present);
    assert_eq!(exit1_obs.exec_creates, 3);
    Ok(())
}
