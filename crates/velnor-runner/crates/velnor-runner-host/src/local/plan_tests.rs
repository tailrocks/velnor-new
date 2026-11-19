// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Plan;
use crate::error::HostError;

fn json(extra: &str) -> Vec<u8> {
    format!(
        "{{\"environment\":\"e\",\"authority\":\"http://127.0.0.1:1\",\"token\":\"{}\",\"docker\":\"unix:///d\",\"platform\":\"linux/arm64\",\"runner_image\":\"sha256:{}\",\"dind_image\":\"sha256:{}\",\"limits\":{{\"runner_cpu_millicores\":1500,\"runner_memory_bytes\":1073741824,\"dind_cpu_millicores\":500,\"dind_memory_bytes\":1073741824}},\"sides\":[{{\"side\":\"trusted\",\"network\":\"n\",\"pairs\":1}}]{extra}}}",
        "t".repeat(40),
        "a".repeat(64),
        "b".repeat(64)
    )
    .into_bytes()
}

#[test]
fn local_ci_is_absent_by_default_and_an_absolute_path_when_given() {
    assert_eq!(Plan::parse(&json("")).expect("plan").local_ci, None);
    let given = Plan::parse(&json(",\"local_ci\":\"/x/velnor-ci\"")).expect("plan");
    assert_eq!(given.local_ci.as_deref(), Some("/x/velnor-ci"));
}

#[test]
fn a_local_ci_that_is_not_a_plain_absolute_path_is_refused() {
    let long = format!("/{}", "a".repeat(4096));
    for value in [
        "\"velnor-ci\"".to_owned(),
        "\"./velnor-ci\"".to_owned(),
        "\"\"".to_owned(),
        "\"/x/\\u0000y\"".to_owned(),
        format!("\"{long}\""),
        "7".to_owned(),
    ] {
        let outcome = Plan::parse(&json(&format!(",\"local_ci\":{value}")));
        assert!(matches!(outcome, Err(HostError::Config)), "{value}");
    }
    let unknown = Plan::parse(&json(",\"local_cii\":\"/x\""));
    assert!(matches!(unknown, Err(HostError::Config)));
}
