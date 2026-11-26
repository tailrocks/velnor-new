// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use velnor_runner_host::{
    ConnectionId, ConnectionPats, DaemonLock, HostConfig, RustRunnerLauncher, run_rust_runner_host,
};
use zeroize::Zeroizing;

use super::fixtures::TestResult;

pub(super) fn run(root: &Path) -> TestResult<()> {
    let state = root.join("state");
    let config = HostConfig::parse(&std::fs::read_to_string(state.join("host.toml"))?)?;
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
