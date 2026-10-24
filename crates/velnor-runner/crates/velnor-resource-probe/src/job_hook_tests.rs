use std::{
    fs::{self, File},
    os::unix::fs::PermissionsExt,
    time::Instant,
};

use rustix::{
    fs::{Mode, OFlags, open},
    process::geteuid,
};

use super::{channel, context, validation, *};

struct TestDirectory {
    path: std::path::PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-job-hook-{}-{sequence}", std::process::id()));
        fs::create_dir(&path).expect("create test directory");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("set test directory mode");
        Self { path }
    }

    fn open(&self) -> File {
        File::from(
            open(
                &self.path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .expect("open test directory"),
        )
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.path);
    }
}

fn challenge() -> Challenge {
    Challenge {
        schema_version: 1,
        nonce: "a".repeat(64),
        reservation_id: 11,
        intent_id: 12,
        witness_request_id: 13,
        runner_id: 14,
        runner_name: "runner-14".to_owned(),
        runner_scale_set_id: 15,
        runner_container_id: "b".repeat(64),
        dind_container_id: "c".repeat(64),
        engine_id: "engine-1".to_owned(),
        docker_root_digest: "d".repeat(64),
        instance_binding_digest: "e".repeat(64),
        policy_generation: 16,
        policy_digest: "f".repeat(64),
        channel_digest: "1".repeat(64),
    }
}

fn request(challenge: &Challenge) -> SourceRequest {
    SourceRequest {
        schema_version: 1,
        challenge: challenge.nonce.clone(),
        reservation_id: challenge.reservation_id,
        intent_id: challenge.intent_id,
        repository_id: 9876,
        repository_name: "owner/repository".to_owned(),
        event_name: "pull_request".to_owned(),
        event_ref: "refs/pull/42/merge".to_owned(),
        tested_sha: "2".repeat(40),
        run_id: 17_000_000_001,
        run_attempt: 2,
        job_key: "required".to_owned(),
        workflow_ref: "owner/repository/.github/workflows/ci.yml@refs/pull/42/merge".to_owned(),
        workflow_sha: "3".repeat(40),
        base_ref: Some("sea/integration".to_owned()),
        base_sha: Some("4".repeat(40)),
        head_ref: Some("build/velnor-actions-0.1.9".to_owned()),
        head_sha: Some("5".repeat(40)),
        pull_request_number: Some(42),
    }
}

fn decision(challenge: &Challenge, request: &SourceRequest) -> Decision {
    let request_bytes =
        validation::canonical_json(request, REQUEST_LIMIT, "test_request").expect("request bytes");
    Decision {
        schema_version: 1,
        allow: true,
        challenge: challenge.nonce.clone(),
        request_sha256: validation::sha256(&request_bytes),
        reservation_id: challenge.reservation_id,
        intent_id: challenge.intent_id,
        actual_request_id: 17,
        run_id: request.run_id,
        run_attempt: request.run_attempt,
        job_key: request.job_key.clone(),
        runner_id: challenge.runner_id,
        policy_generation: challenge.policy_generation,
        policy_digest: challenge.policy_digest.clone(),
        native_job_id: Some("native-job-1".to_owned()),
        rest_job_id: Some("9001".to_owned()),
        deny_code: None,
    }
}

#[test]
fn approved_decision_matches_exact_challenge_request_and_source() {
    let challenge = challenge();
    let request = request(&challenge);
    let expected_digest = validation::sha256(
        &validation::canonical_json(&request, REQUEST_LIMIT, "test_request")
            .expect("request bytes"),
    );
    let accepted = decision(&challenge, &request);
    assert!(
        validation::validate_decision(&challenge, &request, &accepted, &expected_digest).is_ok()
    );

    let mut cases = Vec::new();
    let mut wrong_nonce = accepted.clone();
    wrong_nonce.challenge = "9".repeat(64);
    cases.push(wrong_nonce);
    let mut wrong_digest = accepted.clone();
    wrong_digest.request_sha256 = "8".repeat(64);
    cases.push(wrong_digest);
    let mut wrong_attempt = accepted.clone();
    wrong_attempt.run_attempt += 1;
    cases.push(wrong_attempt);
    let mut wrong_policy = accepted.clone();
    wrong_policy.policy_generation += 1;
    cases.push(wrong_policy);
    let mut wrong_policy_digest = accepted.clone();
    wrong_policy_digest.policy_digest = "7".repeat(64);
    cases.push(wrong_policy_digest);
    let mut wrong_reservation = accepted.clone();
    wrong_reservation.reservation_id += 1;
    cases.push(wrong_reservation);
    let mut wrong_intent = accepted.clone();
    wrong_intent.intent_id += 1;
    cases.push(wrong_intent);
    let mut wrong_runner = accepted.clone();
    wrong_runner.runner_id += 1;
    cases.push(wrong_runner);
    let mut wrong_run = accepted.clone();
    wrong_run.run_id += 1;
    cases.push(wrong_run);
    let mut wrong_job = accepted.clone();
    wrong_job.job_key = "other".to_owned();
    cases.push(wrong_job);
    let mut denied = accepted.clone();
    denied.allow = false;
    denied.deny_code = Some("source_mismatch".to_owned());
    cases.push(denied);
    let mut missing_native = accepted.clone();
    missing_native.native_job_id = None;
    cases.push(missing_native);
    let mut missing_rest = accepted.clone();
    missing_rest.rest_job_id = None;
    cases.push(missing_rest);
    let mut invalid_native = accepted.clone();
    invalid_native.native_job_id = Some(String::new());
    cases.push(invalid_native);
    let mut invalid_request_id = accepted.clone();
    invalid_request_id.actual_request_id = 0;
    cases.push(invalid_request_id);

    for candidate in cases {
        assert!(
            validation::validate_decision(&challenge, &request, &candidate, &expected_digest)
                .is_err()
        );
    }
}

#[test]
fn challenge_rejects_invalid_identity_digest_and_unknown_fields() {
    let mut invalid = challenge();
    invalid.reservation_id = 0;
    assert!(validation::validate_challenge(&invalid).is_err());
    let mut invalid = challenge();
    invalid.nonce = "A".repeat(64);
    assert!(validation::validate_challenge(&invalid).is_err());
    let mut invalid = challenge();
    invalid.policy_digest = "g".repeat(64);
    assert!(validation::validate_challenge(&invalid).is_err());

    let mut encoded = serde_json::to_value(challenge()).expect("challenge JSON");
    encoded["unreviewed"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Challenge>(encoded).is_err());
    assert!(serde_json::from_str::<Challenge>(r#"{"schema_version":1}"#).is_err());
}

#[test]
fn workflow_path_requires_the_exact_repository_and_workflow_tree() {
    assert_eq!(
        context::workflow_path(
            "owner/repository/.github/workflows/ci.yml@refs/heads/sea/integration",
            "owner/repository"
        )
        .expect("workflow path"),
        ".github/workflows/ci.yml"
    );
    assert!(
        context::workflow_path(
            "other/repository/.github/workflows/ci.yml@refs/heads/main",
            "owner/repository"
        )
        .is_err()
    );
    assert!(
        context::workflow_path(
            "owner/repository/../../secret.yml@refs/heads/main",
            "owner/repository"
        )
        .is_err()
    );
    assert!(
        context::workflow_path(
            "owner/repository/ci.yml@refs/heads/main",
            "owner/repository"
        )
        .is_err()
    );
    assert!(channel::open_trusted_directory("relative/path", 0, false).is_err());
    assert!(channel::open_trusted_directory("/opt/../tmp", 0, false).is_err());
}

#[test]
fn pull_request_facts_reject_missing_or_invalid_fields() {
    let payload = br#"{"number":42,"pull_request":{"base":{"ref":"sea/integration","sha":"4444444444444444444444444444444444444444"},"head":{"ref":"build/change","sha":"5555555555555555555555555555555555555555"}}}"#;
    let facts = context::pull_request_facts(payload).expect("pull request facts");
    assert_eq!(facts.number, 42);
    assert_eq!(facts.base_ref, "sea/integration");
    assert_eq!(facts.head_ref, "build/change");
    assert_eq!(facts.base_sha, "4".repeat(40));
    assert_eq!(facts.head_sha, "5".repeat(40));
    assert!(context::pull_request_facts(br#"{"number":42,"pull_request":{}}"#).is_err());
}

#[test]
fn strict_file_read_rejects_wrong_owner_and_write_permission() {
    let directory = TestDirectory::new();
    let user = geteuid().as_raw();
    let valid = serde_json::to_vec(&challenge()).expect("challenge JSON");
    let path = directory.path.join(CHALLENGE_FILE);
    fs::write(&path, &valid).expect("write challenge");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).expect("set file mode");
    assert_eq!(
        channel::read_json_at::<Challenge>(
            &directory.open(),
            CHALLENGE_FILE,
            user,
            CHALLENGE_LIMIT,
            true
        )
        .expect("read challenge"),
        challenge()
    );
    assert!(
        channel::read_json_at::<Challenge>(
            &directory.open(),
            CHALLENGE_FILE,
            user.saturating_add(1),
            CHALLENGE_LIMIT,
            true
        )
        .is_err()
    );

    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("make writable");
    assert!(
        channel::read_json_at::<Challenge>(
            &directory.open(),
            CHALLENGE_FILE,
            user,
            CHALLENGE_LIMIT,
            true
        )
        .is_err()
    );
}

#[test]
fn strict_file_read_rejects_partial_and_oversized_json() {
    let directory = TestDirectory::new();
    let path = directory.path.join(CHALLENGE_FILE);
    fs::write(&path, b"{\"schema_version\":1").expect("write partial");
    set_readonly(&path);
    assert!(
        channel::read_json_at::<Challenge>(
            &directory.open(),
            CHALLENGE_FILE,
            geteuid().as_raw(),
            CHALLENGE_LIMIT,
            true
        )
        .is_err()
    );

    fs::remove_file(&path).expect("remove partial file");
    fs::write(&path, vec![b'x'; CHALLENGE_LIMIT + 1]).expect("write large file");
    set_readonly(&path);
    assert!(
        channel::read_json_at::<Challenge>(
            &directory.open(),
            CHALLENGE_FILE,
            geteuid().as_raw(),
            CHALLENGE_LIMIT,
            true
        )
        .is_err()
    );
}

#[test]
fn strict_file_read_rejects_symlink() {
    let directory = TestDirectory::new();
    std::os::unix::fs::symlink("missing-target", directory.path.join(CHALLENGE_FILE))
        .expect("create symlink");
    assert!(
        channel::read_json_at::<Challenge>(
            &directory.open(),
            CHALLENGE_FILE,
            geteuid().as_raw(),
            CHALLENGE_LIMIT,
            true
        )
        .is_err()
    );
}

fn set_readonly(path: &std::path::Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o444)).expect("set file mode");
}

#[test]
fn request_publish_is_create_once_and_keeps_canonical_bytes() {
    let directory = TestDirectory::new();
    let request = request(&challenge());
    let bytes =
        validation::canonical_json(&request, REQUEST_LIMIT, "test_request").expect("request bytes");
    assert_eq!(bytes, br#"{"schema_version":1,"challenge":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","reservation_id":11,"intent_id":12,"repository_id":9876,"repository_name":"owner/repository","event_name":"pull_request","event_ref":"refs/pull/42/merge","tested_sha":"2222222222222222222222222222222222222222","run_id":17000000001,"run_attempt":2,"job_key":"required","workflow_ref":"owner/repository/.github/workflows/ci.yml@refs/pull/42/merge","workflow_sha":"3333333333333333333333333333333333333333","base_ref":"sea/integration","base_sha":"4444444444444444444444444444444444444444","head_ref":"build/velnor-actions-0.1.9","head_sha":"5555555555555555555555555555555555555555","pull_request_number":42}"#);
    let opened = directory.open();
    channel::write_request_at(&opened, &directory.path, &bytes, &request.challenge)
        .expect("publish request");
    assert_eq!(
        fs::read(directory.path.join(REQUEST_FILE)).expect("read request"),
        bytes
    );
    assert!(
        !directory
            .path
            .join(format!("request.{}.tmp", request.challenge))
            .exists()
    );
    assert!(
        channel::write_request_at(&opened, &directory.path, &bytes, &request.challenge).is_err()
    );
}

#[test]
fn expired_decision_deadline_fails_closed() {
    let directory = TestDirectory::new();
    let deadline = Instant::now() + std::time::Duration::from_millis(2);
    assert!(channel::wait_for_decision(&directory.open(), &directory.path, deadline).is_err());
}

#[test]
fn request_bytes_are_bounded_and_canonical() {
    let request = request(&challenge());
    let bytes =
        validation::canonical_json(&request, REQUEST_LIMIT, "test_request").expect("request bytes");
    assert_eq!(
        serde_json::from_slice::<SourceRequest>(&bytes).expect("decode request"),
        request
    );
    assert!(validation::canonical_json(&request, 1, "test_request").is_err());
    let digest = validation::sha256(&bytes);
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
}
