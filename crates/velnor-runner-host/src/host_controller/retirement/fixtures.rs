// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use crate::config::{HostConfig, RepositoryConnection};

/// Writes the marker, binding snapshot and drain proof the CLI leaves when it removes a
/// connection, so tests can start from a removed connection with a current drain proof.
pub(crate) fn removed_fixture(state: &Path, binding: &RepositoryConnection) {
    use std::os::unix::fs::PermissionsExt;
    const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    let directory = state.join("connection-drain");
    fs::create_dir_all(&directory).expect("drain directory");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).expect("drain mode");
    let snapshot = binding.canonical_identity_bytes().expect("identity");
    let mut proof = format!("{TOKEN}\n").into_bytes();
    proof.extend_from_slice(&snapshot);
    let id = binding.connection_id.as_str();
    for (name, bytes) in [
        (id.to_owned(), TOKEN.as_bytes()),
        (format!("{id}~binding"), snapshot.as_slice()),
        (format!("{id}~drained"), proof.as_slice()),
    ] {
        let path = directory.join(name);
        fs::write(&path, bytes).expect("drain file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("drain file mode");
    }
}

const FIXTURE_CONNECTION: &str = r#"[[connections]]
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
"#;

const FIXTURE_HOST: &str = r#"schema = 2
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

/// The host config holding the single fixture connection `id`.
pub(crate) fn fixture_config(id: &str) -> Result<HostConfig, Box<dyn std::error::Error>> {
    let connection = FIXTURE_CONNECTION.replace("conn-test-1", id);
    Ok(HostConfig::parse(&FIXTURE_HOST.replacen(
        "[host]",
        &format!("{connection}[host]"),
        1,
    ))?)
}
