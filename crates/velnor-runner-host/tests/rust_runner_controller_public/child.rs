// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use std::io::Write;
use std::path::PathBuf;

use velnor_runner_host::{
    ConnectionId, ConnectionPats, DaemonLock, HostConfig, HostError, RustRunnerLauncher,
    SecretSource, install_rust_runner_launcher, install_secret_source, run_host_blocking,
    run_rust_runner_host,
};
use zeroize::Zeroizing;

use super::fixtures::{TestResult, host_toml};

pub(super) const PASSED_MAX_JOBS: &str = "VELNOR_CHILD_PASSED_MAX_JOBS";
pub(super) const HOST_BLOCKING: &str = "VELNOR_CHILD_HOST_BLOCKING";
pub(super) const ORB_DEADLINE_MS: &str = "VELNOR_CHILD_ORB_DEADLINE_MS";
pub(super) const SERVED_SECRET: &str = "VELNOR_CHILD_SERVED_SECRET";
pub(super) const HOLD_STORE: &str = "VELNOR_CHILD_HOLD_STORE";
pub(super) const SECRET_LOOKUPS: &str = "secret-lookups";

struct InjectedSecrets {
    served: Option<Vec<u8>>,
    lookups: PathBuf,
}

impl SecretSource for InjectedSecrets {
    fn load(&self, service: &str, account: &str) -> Result<Zeroizing<Vec<u8>>, HostError> {
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.lookups)
            .map_err(|_| HostError::Keychain)?;
        writeln!(log, "{service}/{account}").map_err(|_| HostError::Keychain)?;
        self.served
            .clone()
            .map(Zeroizing::new)
            .ok_or(HostError::Keychain)
    }
}

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
    let mut launcher = RustRunnerLauncher::new(root.join("orb"), root.join("store"));
    let _held = std::env::var_os(HOLD_STORE)
        .map(|_| launcher.hold_host())
        .transpose()?;
    if let Ok(milliseconds) = std::env::var(ORB_DEADLINE_MS) {
        launcher = launcher.with_orb_deadline(Duration::from_millis(milliseconds.parse()?));
    }
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
    let root = state.parent().ok_or("the state directory has no parent")?;
    let served = std::env::var_os(SERVED_SECRET)
        .map(|path| std::fs::read(root.join(path)))
        .transpose()?;
    install_rust_runner_launcher(RustRunnerLauncher::new(
        root.join("orb"),
        root.join("store"),
    ))?;
    install_secret_source(Box::new(InjectedSecrets {
        served,
        lookups: root.join("seen").join(SECRET_LOOKUPS),
    }))?;
    let lock = DaemonLock::try_acquire_daemon(state)?;
    match run_host_blocking(&config, state, &lock) {
        Ok(()) => println!("CONTROLLER_EXIT Ok(())"),
        Err(error) => println!("CONTROLLER_EXIT Err({error})"),
    }
    Ok(())
}
