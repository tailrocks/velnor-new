// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_runner_host::{DaemonLock, HostConfig, HostError, RustRunnerFailure, run_host_blocking};

const RUST_RUNNER_CONFIG: &str = "schema = 2\n\
    [[connections]]\n\
    connection_id = \"lab\"\n\
    repository_id = 42\n\
    repository = \"tailrocks/velnor-lab\"\n\
    credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
    [connections.scale_set]\n\
    name = \"velnor-mac\"\n\
    labels = [\"velnor\", \"velnor-mac\"]\n\
    [connections.source_policy]\n\
    mode = \"branch_restricted\"\n\
    branches = [\"main\"]\n\
    tag_prefixes = []\n\
    events = [\"push\"]\n\
    pull_request_rules = []\n\
    [host]\n\
    launch_mode = \"rust-runner\"\n\
    max_jobs = 1\n\
    [host.resources]\n\
    runner_cpu_millicores = 2000\n\
    runner_memory_bytes = 4294967296\n\
    dind_cpu_millicores = 1000\n\
    dind_memory_bytes = 2147483648\n\
    operational_memory_reserve_bytes = 1073741824\n\
    [docker]\n\
    context = \"orbstack\"\n\
    platform = \"linux/arm64\"\n\
    endpoint = \"unix:///var/run/docker.sock\"\n";

#[test]
fn the_controller_refuses_rust_runner_mode_until_it_can_start_runners() -> Result<(), Box<dyn Error>>
{
    let state = tempfile::tempdir()?;
    let lock = DaemonLock::try_acquire_daemon(state.path())?;
    let config = HostConfig::parse(RUST_RUNNER_CONFIG)?;
    let error = run_host_blocking(&config, state.path(), &lock)
        .err()
        .ok_or("the controller started in rust-runner mode")?;
    assert_eq!(
        error.to_string(),
        HostError::RustRunner(RustRunnerFailure::ControllerUnavailable).to_string()
    );
    Ok(())
}
