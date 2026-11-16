// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::{API_DEFAULT_VERSION, Docker};

use super::{Plan, supervise};
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
