// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use velnor_runner_host::{
    ConnectionId, HostConfig, HostError, LaunchMode, SecretStore, build_job_authority_policy_json,
    store_secret,
};

use super::connect::{ConnectError, ConnectRequest, connect_with_store, persist_reconnected};
use super::connection_state::{connection_absence_record, connection_drain_marker};
use super::marker::write_marker;

pub(super) const SERVICE: &str = "com.tailrocks.velnor.test";
pub(super) const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const INVENTORY: &str = r#"{"schema_version":1,"generator_source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","inventory_sha256":"874cb3ddb3852071a5d20fa24e750e88cd21050a0206fbd9b30dd35fad143b32","workflows":[{"path":".github/workflows/ci.yml","content_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","calls":[],"jobs":[{"root_path":".github/workflows/ci.yml","workflow_path":".github/workflows/ci.yml","job_key":"build","role":"verification","contexts":[{"event":"push","branch_filters":["main"],"tag_filters":[],"pull_request_base_branches":[]}],"permissions_declared":true,"permissions":{"contents":"read"},"secrets":[],"environments":[],"condition_sha256":null,"concurrency":null,"needs":[]}]}]}"#;

#[derive(Default)]
pub(super) struct FakeKeychain(RefCell<HashMap<(String, String), Vec<u8>>>);

impl SecretStore for FakeKeychain {
    fn store(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), HostError> {
        self.0
            .borrow_mut()
            .insert((service.to_owned(), account.to_owned()), secret.to_vec());
        Ok(())
    }
}

impl FakeKeychain {
    fn stored(&self) -> Vec<Vec<u8>> {
        self.0.borrow().values().cloned().collect()
    }
}

pub(super) fn request<'a>(state: &'a Path, policy: Option<&'a Path>) -> ConnectRequest<'a> {
    ConnectRequest {
        state,
        connection_id: "conn-test-1",
        repo: "owner/repo",
        repository_id: 42,
        job_authority_policy: policy,
        job_authority_inventory: None,
        job_authority_generator_source_sha: None,
        job_authority_generator_artifact_sha256: None,
        job_authority_generation: None,
        scale_set: "ubuntu-26.04-scale-set",
        scale_set_labels: vec!["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
        source_policy_mode: "branch-restricted",
        events: vec!["push".to_owned()],
        branches: vec!["main".to_owned()],
        tag_prefixes: Vec::new(),
        pull_request_rules: Vec::new(),
        organization: None,
        runner_group: None,
        platform: "linux/amd64",
        launch_mode: LaunchMode::OfficialRunner,
        max_jobs: Some(1),
        docker_context: Some("orbstack"),
        endpoint: "unix:///var/run/docker.sock",
        runner_cpu_millicores: 1000,
        runner_memory_bytes: 1_073_741_824,
        dind_cpu_millicores: 1000,
        dind_memory_bytes: 1_073_741_824,
        operational_memory_reserve_bytes: 1_073_741_824,
    }
}

pub(super) fn connect(
    keychain: &FakeKeychain,
    request: &ConnectRequest<'_>,
    token: &str,
) -> Result<(), ConnectError> {
    connect_with_store(
        &mut token.as_bytes(),
        SERVICE,
        request,
        |service, account, secret| store_secret(keychain, service, account, secret),
    )
}

fn policy_file(
    state: &Path,
    name: &str,
    generation: u64,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let config = HostConfig::parse(&std::fs::read_to_string(state.join("host.toml"))?)?;
    let connection = config
        .connection(&ConnectionId::parse("conn-test-1")?)
        .ok_or("connection missing")?;
    let json = build_job_authority_policy_json(
        connection,
        INVENTORY,
        SOURCE_SHA,
        &"b".repeat(64),
        generation,
    )?;
    let path = state.join(name);
    std::fs::write(&path, json)?;
    Ok(path)
}

#[test]
fn repeat_connect_with_the_same_job_authority_policy_stores_the_new_credential()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let keychain = FakeKeychain::default();
    assert_eq!(
        connect(&keychain, &request(&state, None), "token-0"),
        Ok(())
    );
    let policy = policy_file(&state, "policy.json", 1)?;

    assert_eq!(
        connect(&keychain, &request(&state, Some(&policy)), "token-1"),
        Ok(())
    );
    assert_eq!(keychain.stored(), vec![b"token-1".to_vec()]);
    let configured = std::fs::read_to_string(state.join("host.toml"))?;

    assert_eq!(
        connect(&keychain, &request(&state, Some(&policy)), "token-2"),
        Ok(())
    );
    assert_eq!(keychain.stored(), vec![b"token-2".to_vec()]);
    assert_eq!(
        std::fs::read_to_string(state.join("host.toml"))?,
        configured
    );
    Ok(())
}

#[test]
fn repeat_connect_with_a_different_job_authority_policy_is_rejected_without_storing_the_credential()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let keychain = FakeKeychain::default();
    assert_eq!(
        connect(&keychain, &request(&state, None), "token-0"),
        Ok(())
    );
    let first = policy_file(&state, "first.json", 1)?;
    let second = policy_file(&state, "second.json", 2)?;
    assert_eq!(
        connect(&keychain, &request(&state, Some(&first)), "token-1"),
        Ok(())
    );
    let configured = std::fs::read_to_string(state.join("host.toml"))?;

    let rejected = connect(&keychain, &request(&state, Some(&second)), "token-2");
    assert_eq!(rejected, Err(ConnectError::Config));
    assert_eq!(keychain.stored(), vec![b"token-1".to_vec()]);
    assert_eq!(
        std::fs::read_to_string(state.join("host.toml"))?,
        configured
    );
    Ok(())
}

const P5_REPOSITORY_ID: i64 = 1_390_620_900;

fn p5_request(state: &Path) -> ConnectRequest<'_> {
    ConnectRequest {
        connection_id: "velnor-new",
        repo: "tailrocks/velnor-new",
        repository_id: P5_REPOSITORY_ID,
        scale_set: "ubuntu-26.04-velnor-scale-set",
        scale_set_labels: vec![
            "velnor".to_owned(),
            "ubuntu-26.04-velnor-scale-set".to_owned(),
        ],
        events: vec!["push".to_owned(), "pull_request".to_owned()],
        branches: vec!["p5/velnor-host-canary".to_owned()],
        pull_request_rules: vec![format!(
            "*:{P5_REPOSITORY_ID}:main:{P5_REPOSITORY_ID}:p5/velnor-host-canary"
        )],
        organization: Some("tailrocks"),
        runner_group: Some("velnor-ci"),
        max_jobs: Some(2),
        ..request(state, None)
    }
}

#[test]
fn connect_writes_launch_mode_rust_runner() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let keychain = FakeKeychain::default();
    let inventory = state.join("inventory.json");
    std::fs::write(&inventory, INVENTORY)?;
    let artifact = "b".repeat(64);
    let request = ConnectRequest {
        scale_set: "velnor-mac",
        scale_set_labels: vec!["velnor".to_owned(), "velnor-mac".to_owned()],
        platform: "linux/arm64",
        launch_mode: LaunchMode::RustRunner,
        job_authority_inventory: Some(&inventory),
        job_authority_generator_source_sha: Some(SOURCE_SHA),
        job_authority_generator_artifact_sha256: Some(&artifact),
        job_authority_generation: Some(1),
        ..request(&state, None)
    };
    assert_eq!(connect(&keychain, &request, "token-0"), Ok(()));

    let text = std::fs::read_to_string(state.join("host.toml"))?;
    assert!(text.contains("launch_mode = \"rust-runner\""), "{text}");
    let config = HostConfig::parse(&text)?;
    assert_eq!(config.host.launch_mode, LaunchMode::RustRunner);
    switching_the_launch_mode_is_rejected(&inventory, &artifact)
}

fn arm_request<'a>(
    state: &'a Path,
    launch_mode: LaunchMode,
    inventory: &'a Path,
    artifact: &'a str,
) -> ConnectRequest<'a> {
    ConnectRequest {
        scale_set: "ubuntu-26.04-arm-scale-set",
        scale_set_labels: vec!["velnor".to_owned(), "ubuntu-26.04-arm-scale-set".to_owned()],
        platform: "linux/arm64",
        launch_mode,
        job_authority_inventory: Some(inventory),
        job_authority_generator_source_sha: Some(SOURCE_SHA),
        job_authority_generator_artifact_sha256: Some(artifact),
        job_authority_generation: Some(1),
        ..request(state, None)
    }
}

fn switching_the_launch_mode_is_rejected(
    inventory: &Path,
    artifact: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for mode in [LaunchMode::RustRunner, LaunchMode::OfficialRunner] {
        let temp = tempfile::tempdir()?;
        let state = temp.path().canonicalize()?;
        let keychain = FakeKeychain::default();
        let request = arm_request(&state, mode, inventory, artifact);
        assert_eq!(connect(&keychain, &request, "token-0"), Ok(()), "{mode:?}");
    }
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let keychain = FakeKeychain::default();
    let first = arm_request(&state, LaunchMode::RustRunner, inventory, artifact);
    assert_eq!(
        connect(&keychain, &first, "token-1"),
        Ok(()),
        "the first connect in rust-runner mode must succeed"
    );
    let configured = std::fs::read(state.join("host.toml"))?;
    let second = arm_request(&state, LaunchMode::OfficialRunner, inventory, artifact);
    assert_eq!(
        connect(&keychain, &second, "token-2"),
        Err(ConnectError::Rejected),
        "a connect that switches the launch mode must be rejected"
    );
    assert_eq!(
        std::fs::read(state.join("host.toml"))?,
        configured,
        "a rejected connect must leave host.toml byte-identical"
    );
    assert_eq!(
        keychain.stored(),
        vec![b"token-1".to_vec()],
        "a rejected connect must not store its token"
    );
    Ok(())
}

#[test]
fn connect_defaults_launch_mode_to_official_runner() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let keychain = FakeKeychain::default();
    assert_eq!(
        connect(&keychain, &request(&state, None), "token-0"),
        Ok(())
    );

    let text = std::fs::read_to_string(state.join("host.toml"))?;
    assert!(text.contains("launch_mode = \"official-runner\""), "{text}");
    let config = HostConfig::parse(&text)?;
    assert_eq!(config.host.launch_mode, LaunchMode::OfficialRunner);
    Ok(())
}

#[test]
fn the_p5_org_scale_set_connection_is_accepted_and_recorded()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let keychain = FakeKeychain::default();
    let request = p5_request(&state);
    assert_eq!(connect(&keychain, &request, "token-0"), Ok(()));

    let config = HostConfig::parse(&std::fs::read_to_string(state.join("host.toml"))?)?;
    let connection = config
        .connection(&ConnectionId::parse("velnor-new")?)
        .ok_or("connection missing")?;
    assert_eq!(connection.organization.as_deref(), Some("tailrocks"));
    assert_eq!(connection.runner_group.as_deref(), Some("velnor-ci"));
    assert_eq!(connection.repository, "tailrocks/velnor-new");
    assert_eq!(connection.repository_id, P5_REPOSITORY_ID);
    assert_eq!(connection.scale_set.name(), "ubuntu-26.04-velnor-scale-set");
    assert_eq!(
        connection.scale_set.labels(),
        ["velnor", "ubuntu-26.04-velnor-scale-set"]
    );

    assert_eq!(connect(&keychain, &request, "token-1"), Ok(()));
    assert_eq!(keychain.stored(), vec![b"token-1".to_vec()]);
    Ok(())
}

#[test]
fn a_runner_group_needs_an_organization_that_owns_the_repository()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let keychain = FakeKeychain::default();
    let no_org = ConnectRequest {
        organization: None,
        ..p5_request(&state)
    };
    assert_eq!(
        connect(&keychain, &no_org, "token-0"),
        Err(ConnectError::Config)
    );
    let foreign = ConnectRequest {
        organization: Some("someone-else"),
        ..p5_request(&state)
    };
    assert_eq!(
        connect(&keychain, &foreign, "token-0"),
        Err(ConnectError::Config)
    );
    assert_eq!(keychain.stored(), Vec::<Vec<u8>>::new());
    assert!(!state.join("host.toml").exists());
    Ok(())
}

#[test]
fn reconnecting_removes_the_github_absence_record() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().canonicalize()?;
    let keychain = FakeKeychain::default();
    connect(&keychain, &request(&state, None), "token").map_err(|_| "connect")?;
    let text = std::fs::read_to_string(state.join("host.toml"))?;
    let id = ConnectionId::parse("conn-test-1")?;
    let config = HostConfig::parse(&text)?;
    let connection = config.connection(&id).ok_or("connection missing")?;
    let marker = connection_drain_marker(&state, &id);
    let token = write_marker(&marker).map_err(|_| "marker")?;
    let record = connection_absence_record(&state, &id);
    std::fs::write(&record, "token\n")?;
    persist_reconnected(&state, &text, connection, &marker, &token).map_err(|_| "reconnect")?;
    assert!(!record.exists());
    Ok(())
}
