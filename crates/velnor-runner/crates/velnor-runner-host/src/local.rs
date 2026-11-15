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
    ENVIRONMENT_LABEL, LocalPair, START_GRACE, StartFailure, remove_pair, remove_pair_named,
    start_pair,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnePairError {
    pub error: HostError,
    pub leftover: Option<String>,
}

impl std::fmt::Display for OnePairError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.error)?;
        match &self.leftover {
            Some(name) => write!(
                formatter,
                "; pair {name} may be left behind: remove the containers and volumes labelled {}={name}",
                pair::PAIR_LABEL
            ),
            None => Ok(()),
        }
    }
}

impl std::error::Error for OnePairError {}

impl From<HostError> for OnePairError {
    fn from(error: HostError) -> Self {
        Self {
            error,
            leftover: None,
        }
    }
}

fn remove_or_name(
    runtime: &tokio::runtime::Runtime,
    docker: &bollard::Docker,
    name: &str,
    error: HostError,
) -> OnePairError {
    let leftover = runtime
        .block_on(remove_pair_named(docker, name))
        .err()
        .map(|_| name.to_owned());
    OnePairError { error, leftover }
}

pub fn run_one_pair(input: &mut impl BufRead, output: &mut impl Write) -> Result<(), OnePairError> {
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
        .map_err(|failure| match failure.leftover {
            Some(name) => remove_or_name(&runtime, &docker, &name, failure.error),
            None => failure.error.into(),
        })?;
    let started = writeln!(
        output,
        "pair {} started on {}",
        pair.name(),
        request.pair.network
    )
    .and_then(|()| output.flush());
    if started.is_err() {
        return Err(remove_or_name(
            &runtime,
            &docker,
            pair.name(),
            HostError::Path,
        ));
    }
    let drained = std::io::copy(input, &mut std::io::sink());
    let removed = runtime
        .block_on(remove_pair(&docker, &pair))
        .map_err(|error| OnePairError {
            error,
            leftover: Some(pair.name().to_owned()),
        })?;
    drained.map_err(|_| HostError::Path)?;
    writeln!(
        output,
        "pair {} removed: {} containers, {} volumes",
        pair.name(),
        removed.containers,
        removed.volumes
    )
    .map_err(|_| HostError::Path)?;
    Ok(())
}
