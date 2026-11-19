// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::{HostConfig, connect_unix};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

#[test]
fn connect_unix_never_rewrites_the_named_socket_path() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("docker.sock");
    std::fs::write(&socket, b"")?;
    let base = directory.path().display();
    assert!(connect_unix(&format!("{base}/unix://docker.sock")).is_err());
    assert!(connect_unix(&format!("unix://{base}/unix://docker.sock")).is_err());
    assert!(connect_unix(&format!("unix://{base}/docker.sock")).is_ok());
    assert!(connect_unix(&format!("{base}/docker.sock")).is_ok());
    Ok(())
}

#[test]
fn config_admission_rejects_the_endpoint_connect_unix_rejects() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let base = directory.path().display();
    let with_endpoint = |endpoint: String| CONFIG.replace("unix:///docker.sock", &endpoint);
    assert!(
        HostConfig::parse(&with_endpoint(format!("unix://{base}/unix://docker.sock"))).is_err()
    );
    assert!(HostConfig::parse(&with_endpoint(format!("unix://{base}/docker.sock"))).is_ok());
    Ok(())
}
