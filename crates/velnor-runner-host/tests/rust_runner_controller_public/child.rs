// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use velnor_runner_host::{
    ConnectionId, ConnectionPats, DaemonLock, HostConfig, RustRunnerLauncher, run_host_blocking,
    run_rust_runner_host,
};
use zeroize::Zeroizing;

use super::fixtures::{TestResult, host_toml};

pub(super) const PASSED_MAX_JOBS: &str = "VELNOR_CHILD_PASSED_MAX_JOBS";
pub(super) const HOST_BLOCKING: &str = "VELNOR_CHILD_HOST_BLOCKING";

pub(super) fn run(root: &Path) -> TestResult<()> {
    let state = root.join("state");
    let text = std::fs::read_to_string(state.join("host.toml"))?;
    if std::env::var_os(HOST_BLOCKING).is_some() {
        return blocking(&state, &text);
    }
    let passed = match std::env::var(PASSED_MAX_JOBS) {
        Ok(max_jobs) => host_toml(root, max_jobs.parse()?)?,
        Err(_) => text,
    };
    let config = HostConfig::parse(&passed)?;
    let secret = Zeroizing::new(std::fs::read(root.join("pat"))?);
    let pats = ConnectionPats::from_secrets(&config, [(ConnectionId::parse("lab")?, secret)])?;
    let lock = DaemonLock::try_acquire_daemon(&state)?;
    let launcher = RustRunnerLauncher::new(root.join("orb"), root.join("store"));
    let stop = root.join("seen/stop");
    let shutdown = async move {
        while !stop.exists() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let exit = runtime.block_on(run_rust_runner_host(
        config, pats, &state, &lock, launcher, shutdown,
    ));
    match exit {
        Ok(exit) => println!("CONTROLLER_EXIT Ok({exit:?})"),
        Err(error) => println!("CONTROLLER_EXIT Err({error})"),
    }
    Ok(())
}

fn blocking(state: &Path, text: &str) -> TestResult<()> {
    let config = HostConfig::parse(text)?;
    let lock = DaemonLock::try_acquire_daemon(state)?;
    match run_host_blocking(&config, state, &lock) {
        Ok(()) => println!("CONTROLLER_EXIT Ok(())"),
        Err(error) => println!("CONTROLLER_EXIT Err({error})"),
    }
    Ok(())
}
