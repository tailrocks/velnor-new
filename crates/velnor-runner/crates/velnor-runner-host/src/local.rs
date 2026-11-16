// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::Path;

use tokio::signal::unix::{SignalKind, signal};

use crate::error::HostError;

mod authority;
mod channel;
mod decide;
mod layout;
mod minted;
mod pair;
mod plan;
mod policy;
mod remove;
mod request;
mod slot;
mod storage;
mod supervisor;

pub use pair::{
    ENVIRONMENT_LABEL, LocalPair, START_GRACE, StartFailure, remove_pair, remove_pair_named,
    start_pair,
};
pub use plan::{LoopbackAuthority, MARKER, Plan, Side, check_state};
pub use remove::Removed;
pub use request::LocalPairRequest;
pub use supervisor::supervise;

pub fn run_local(state: &Path, input: impl Read) -> Result<(), HostError> {
    check_state(state)?;
    let plan = Plan::read(input)?;
    let docker = crate::docker_client::connect_unix(&plan.docker)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Docker)?;
    runtime.block_on(async {
        let mut terminate = signal(SignalKind::terminate()).map_err(|_| HostError::Docker)?;
        let mut interrupt = signal(SignalKind::interrupt()).map_err(|_| HostError::Docker)?;
        let shutdown = async {
            tokio::select! {
                _ = terminate.recv() => {}
                _ = interrupt.recv() => {}
            }
        };
        supervise(docker, plan, state, shutdown).await
    })
}
