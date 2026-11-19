// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::{API_DEFAULT_VERSION, Docker};

use super::{Plan, Side, local_ci_for, supervise};
use crate::error::HostError;

fn plan(policy: &str) -> Plan {
    let json = format!(
        "{{\"environment\":\"e\",\"authority\":\"http://127.0.0.1:1\",\"token\":\"{}\",\"docker\":\"unix:///nonexistent\",\"platform\":\"linux/amd64\",\"runner_image\":\"sha256:{}\",\"dind_image\":\"sha256:{}\",\"limits\":{{\"runner_cpu_millicores\":1500,\"runner_memory_bytes\":1073741824,\"dind_cpu_millicores\":500,\"dind_memory_bytes\":1073741824}},\"sides\":[{{\"side\":\"trusted\",\"network\":\"n\",\"pairs\":1}}],\"policy\":\"{policy}\"}}",
        "t".repeat(40),
        "a".repeat(64),
        "b".repeat(64)
    );
    Plan::parse(json.as_bytes()).expect("plan")
}

#[tokio::test]
async fn a_policy_that_does_not_load_stops_the_host_before_any_pair_starts() {
    let state = tempfile::tempdir().expect("state");
    std::fs::write(state.path().join("policy.json"), "not json").expect("write");
    let socket = state.path().join("docker.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("socket");
    for policy in ["policy.json", "missing.json"] {
        let docker =
            Docker::connect_with_unix(socket.to_str().expect("path"), 1, API_DEFAULT_VERSION)
                .expect("docker");
        let outcome = supervise(docker, plan(policy), state.path(), async {}).await;
        assert!(
            matches!(outcome, Err(HostError::Config)),
            "{policy}: {outcome:?}"
        );
        assert!(!state.path().join("decisions.jsonl").exists());
    }
}

#[tokio::test]
async fn a_local_ci_file_that_does_not_load_stops_the_host_before_any_pair_starts() {
    let state = tempfile::tempdir().expect("state");
    let socket = state.path().join("docker.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("socket");
    let docker = Docker::connect_with_unix(socket.to_str().expect("path"), 1, API_DEFAULT_VERSION)
        .expect("docker");
    let mut planned = plan("policy.json");
    planned.policy = None;
    planned.platform = "linux/arm64".to_owned();
    planned.local_ci = Some(state.path().join("missing").display().to_string());
    let outcome = supervise(docker, planned, state.path(), async {}).await;
    assert!(matches!(outcome, Err(HostError::Config)), "{outcome:?}");
    assert!(!state.path().join("decisions.jsonl").exists());
}

#[test]
fn only_the_trusted_side_is_given_the_local_ci_path() {
    let mut planned = plan("policy.json");
    assert_eq!(local_ci_for(&planned, Side::Trusted), None);
    planned.local_ci = Some("/x/velnor-ci".to_owned());
    assert_eq!(
        local_ci_for(&planned, Side::Trusted).as_deref(),
        Some("/x/velnor-ci")
    );
    assert_eq!(local_ci_for(&planned, Side::Untrusted), None);
}
