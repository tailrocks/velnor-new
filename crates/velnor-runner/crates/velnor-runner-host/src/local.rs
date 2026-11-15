// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, Read, Write};

use serde::Deserialize;

use crate::error::HostError;

mod layout;
mod pair;
mod remove;
mod request;
mod storage;

pub use pair::{
    ENVIRONMENT_LABEL, LocalPair, StartFailure, remove_pair, remove_pair_named, start_pair,
};
pub use remove::Removed;
pub use request::LocalPairRequest;

const MAX_REQUEST_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OnePair {
    docker: String,
    pair: LocalPairRequest,
}

pub fn run_one_pair(input: &mut impl BufRead, output: &mut impl Write) -> Result<(), HostError> {
    let mut line = String::new();
    input
        .by_ref()
        .take(MAX_REQUEST_BYTES)
        .read_line(&mut line)
        .map_err(|_| HostError::Config)?;
    let request: OnePair = serde_json::from_str(&line).map_err(|_| HostError::Config)?;
    let docker = crate::docker_client::connect_unix(&request.docker)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Docker)?;
    let pair = runtime
        .block_on(start_pair(&docker, &request.pair))
        .map_err(|failure| {
            if let Some(name) = &failure.leftover {
                let _retried = runtime.block_on(remove_pair_named(&docker, name));
            }
            failure.error
        })?;
    let started = writeln!(
        output,
        "pair {} started on {}",
        pair.name(),
        request.pair.network
    )
    .and_then(|()| output.flush());
    if started.is_err() {
        runtime.block_on(remove_pair(&docker, &pair))?;
        return Err(HostError::Path);
    }
    let drained = std::io::copy(input, &mut std::io::sink());
    let removed = runtime.block_on(remove_pair(&docker, &pair))?;
    drained.map_err(|_| HostError::Path)?;
    writeln!(
        output,
        "pair {} removed: {} containers, {} volumes",
        pair.name(),
        removed.containers,
        removed.volumes
    )
    .map_err(|_| HostError::Path)
}
