// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs::{self, File};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use velnor_resource_probe::ProbeError;
use velnor_resource_probe::execution_request::{
    REQUEST_FILE, RequestMetadata, ensure_stable_content, ensure_stable_identity, read_request_file,
};
use velnor_resource_probe::job_hook::{
    Challenge, Decision, SourceRequest, canonical_json, sha256, validate_challenge,
    validate_decision,
};

type TestResult<T> = Result<T, Box<dyn Error>>;

fn hex_64(ch: char) -> String {
    ch.to_string().repeat(64)
}

fn hex_40(ch: char) -> String {
    ch.to_string().repeat(40)
}

fn sample_challenge() -> Challenge {
    Challenge {
        schema_version: 1,
        nonce: hex_64('a'),
        reservation_id: 11,
        intent_id: 22,
        witness_request_id: 33,
        runner_id: 44,
        runner_name: "velnor-runner-1".to_owned(),
        runner_scale_set_id: 55,
        runner_container_id: hex_64('b'),
        dind_container_id: hex_64('c'),
        engine_id: "engine-1".to_owned(),
        docker_root_digest: hex_64('d'),
        instance_binding_digest: hex_64('e'),
        policy_generation: 1,
        policy_digest: hex_64('f'),
        channel_digest: hex_64('1'),
    }
}

fn sample_request(challenge: &Challenge) -> SourceRequest {
    SourceRequest {
        schema_version: 1,
        challenge: challenge.nonce.clone(),
        reservation_id: challenge.reservation_id,
        intent_id: challenge.intent_id,
        repository_id: 1_438_742_200,
        repository_name: "donbeave/essential-mise".to_owned(),
        event_name: "pull_request".to_owned(),
        event_ref: "refs/pull/1/merge".to_owned(),
        tested_sha: hex_40('2'),
        run_id: 9001,
        run_attempt: 1,
        job_key: "verify__scale_set".to_owned(),
        workflow_ref: "donbeave/essential-mise/.github/workflows/ci.yml@refs/pull/1/merge"
            .to_owned(),
        workflow_sha: hex_40('3'),
        base_ref: Some("main".to_owned()),
        base_sha: Some(hex_40('4')),
        head_ref: Some("feature".to_owned()),
        head_sha: Some(hex_40('5')),
        pull_request_number: Some(1),
    }
}

fn sample_decision(
    challenge: &Challenge,
    request: &SourceRequest,
    request_sha256: &str,
) -> Decision {
    Decision {
        schema_version: 1,
        allow: true,
        challenge: challenge.nonce.clone(),
        request_sha256: request_sha256.to_owned(),
        reservation_id: challenge.reservation_id,
        intent_id: challenge.intent_id,
        actual_request_id: challenge.witness_request_id,
        run_id: request.run_id,
        run_attempt: request.run_attempt,
        job_key: request.job_key.clone(),
        runner_id: challenge.runner_id,
        policy_generation: challenge.policy_generation,
        policy_digest: challenge.policy_digest.clone(),
        native_job_id: Some("native-job-1".to_owned()),
        rest_job_id: None,
        deny_code: None,
    }
}

#[test]
fn public_job_hook_validates_challenge_and_allows_optional_bounded_rest_job_id() -> TestResult<()> {
    let challenge = sample_challenge();
    validate_challenge(&challenge)?;
    let request = sample_request(&challenge);
    let request_bytes = canonical_json(&request, 16 * 1024, "job_hook_request")?;
    let request_sha256 = sha256(&request_bytes);

    let without_rest_id = sample_decision(&challenge, &request, &request_sha256);
    validate_decision(&challenge, &request, &without_rest_id, &request_sha256)?;

    let mut with_rest_id = without_rest_id.clone();
    with_rest_id.rest_job_id = Some("987654321".to_owned());
    validate_decision(&challenge, &request, &with_rest_id, &request_sha256)?;

    let mut max_bounds = without_rest_id;
    max_bounds.native_job_id = Some("n".repeat(256));
    max_bounds.rest_job_id = Some("r".repeat(128));
    validate_decision(&challenge, &request, &max_bounds, &request_sha256)?;
    Ok(())
}

#[test]
fn public_job_hook_rejects_invalid_rest_job_id_and_decision_mismatches() -> TestResult<()> {
    let challenge = sample_challenge();
    let request = sample_request(&challenge);
    let request_bytes = canonical_json(&request, 16 * 1024, "job_hook_request")?;
    let request_sha256 = sha256(&request_bytes);
    let base = sample_decision(&challenge, &request, &request_sha256);

    for invalid_rest in [
        Some(String::new()),
        Some("r".repeat(129)),
        Some("bad\nrest".to_owned()),
    ] {
        let mut decision = base.clone();
        decision.rest_job_id = invalid_rest;
        assert!(matches!(
            validate_decision(&challenge, &request, &decision, &request_sha256),
            Err(ProbeError::Invalid("job_hook_decision_mismatch"))
        ));
    }

    for invalid_native in [None, Some(String::new()), Some("n".repeat(257))] {
        let mut decision = base.clone();
        decision.native_job_id = invalid_native;
        assert!(matches!(
            validate_decision(&challenge, &request, &decision, &request_sha256),
            Err(ProbeError::Invalid("job_hook_decision_mismatch"))
        ));
    }

    let mut denied = base.clone();
    denied.allow = false;
    assert!(matches!(
        validate_decision(&challenge, &request, &denied, &request_sha256),
        Err(ProbeError::Invalid("job_hook_decision_mismatch"))
    ));

    let mut with_deny_code = base;
    with_deny_code.deny_code = Some("source_policy_rejected".to_owned());
    assert!(matches!(
        validate_decision(&challenge, &request, &with_deny_code, &request_sha256),
        Err(ProbeError::Invalid("job_hook_decision_mismatch"))
    ));
    Ok(())
}

#[test]
fn public_job_hook_rejects_malformed_challenge_fields() {
    let base = sample_challenge();

    let mut wrong_schema = base.clone();
    wrong_schema.schema_version = 2;
    assert!(matches!(
        validate_challenge(&wrong_schema),
        Err(ProbeError::Invalid("job_hook_challenge_identity"))
    ));

    let mut zero_policy = base.clone();
    zero_policy.policy_generation = 0;
    assert!(matches!(
        validate_challenge(&zero_policy),
        Err(ProbeError::Invalid("job_hook_challenge_identity"))
    ));

    let mut uppercase_nonce = base;
    uppercase_nonce.nonce = "A".repeat(64);
    assert!(matches!(
        validate_challenge(&uppercase_nonce),
        Err(ProbeError::Invalid("job_hook_challenge"))
    ));
}

struct ScopedTempDir(std::path::PathBuf);

impl ScopedTempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "velnor-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for ScopedTempDir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn assert_mid_read_stability_contracts(meta: RequestMetadata, payload: &[u8]) -> TestResult<()> {
    ensure_stable_identity(meta, meta, meta, meta)?;
    ensure_stable_content(payload, payload)?;
    let mut swapped_inode = meta;
    swapped_inode.inode = meta.inode.wrapping_add(1);
    assert!(matches!(
        ensure_stable_identity(meta, meta, swapped_inode, swapped_inode),
        Err(ProbeError::Invalid("execution_request_changed"))
    ));
    assert!(matches!(
        ensure_stable_content(payload, b"{\"schema_version\":2}\n"),
        Err(ProbeError::Invalid("execution_request_changed"))
    ));
    Ok(())
}

#[test]
fn public_inbox_request_poll_returns_none_when_published_after_initial_enoent_and_reads_on_next_poll()
-> TestResult<()> {
    let temp = ScopedTempDir::new("inbox-request-poll")?;
    let inbox_path = temp.path().join("inbox");
    fs::create_dir(&inbox_path)?;
    fs::set_permissions(&inbox_path, fs::Permissions::from_mode(0o700))?;
    let inbox = File::open(&inbox_path)?;
    let meta = inbox.metadata()?;
    let (uid, gid) = (meta.uid(), meta.gid());

    let request_path = inbox_path.join(REQUEST_FILE);
    let payload = b"{\"schema_version\":1}\n";
    let mut observed_meta = None;
    for round in 0..64_usize {
        if request_path.exists() {
            fs::remove_file(&request_path)?;
        }
        let staged_path = inbox_path.join(format!(".request.{round}.tmp"));
        fs::write(&staged_path, payload)?;
        fs::set_permissions(&staged_path, fs::Permissions::from_mode(0o600))?;

        let barrier = Arc::new(Barrier::new(2));
        let empty_polls = Arc::new(AtomicUsize::new(0));
        let poller_inbox = inbox.try_clone()?;
        let poller_barrier = Arc::clone(&barrier);
        let poller_polls = Arc::clone(&empty_polls);

        let poller = thread::spawn(move || -> Result<(Vec<u8>, RequestMetadata), ProbeError> {
            poller_barrier.wait();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some((bytes, read_meta)) = read_request_file(&poller_inbox, uid, gid)? {
                    return Ok((bytes, read_meta));
                }
                poller_polls.fetch_add(1, Ordering::Release);
                if Instant::now() >= deadline {
                    return Err(ProbeError::Invalid("test_poll_timeout"));
                }
            }
        });

        barrier.wait();
        let wait_deadline = Instant::now() + Duration::from_secs(5);
        while empty_polls.load(Ordering::Acquire) <= round % 4 {
            if Instant::now() >= wait_deadline {
                return Err("timed out waiting for initial empty poll".into());
            }
            thread::yield_now();
        }
        fs::rename(&staged_path, &request_path)?;

        let (read_bytes, read_meta) = poller
            .join()
            .map_err(|_| "poller thread panicked")?
            .map_err(|err| format!("round {round} failed: {err:?}"))?;
        assert_eq!(read_bytes, payload);
        assert_eq!(read_meta.size, payload.len() as u64);
        observed_meta = Some(read_meta);
    }

    assert_mid_read_stability_contracts(observed_meta.ok_or("missing observed meta")?, payload)?;

    fs::write(&request_path, payload)?;
    fs::set_permissions(&request_path, fs::Permissions::from_mode(0o644))?;
    assert!(matches!(
        read_request_file(&inbox, uid, gid),
        Err(ProbeError::Invalid("execution_request_file"))
    ));
    Ok(())
}
