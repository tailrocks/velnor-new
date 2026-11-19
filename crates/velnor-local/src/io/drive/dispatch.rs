// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use serde_json::Value;

use super::Driver;
use crate::cli::RunArgs;
use crate::io::env::{Addresses, Engine, Environment};
use crate::io::proxy::Proxy;

#[derive(Debug)]
pub struct Event<'a> {
    pub name: &'a str,
    pub payload: &'a Value,
    pub git_ref: &'a str,
    pub sha: &'a str,
}

/// Sends one event the way `run` does, to a runner.server, a cache and an
/// authority that all answer at `server`.
pub async fn dispatch<E: Engine + Copy + Send>(
    args: &RunArgs,
    env: &Environment<E>,
    (server, token): (&str, &str),
    event: &Event<'_>,
) -> io::Result<()> {
    let mut driver = Driver {
        args,
        env,
        addresses: Addresses {
            cache: Proxy::authorized(server, token),
            fork: None,
            stub: Proxy::authorized(server, token),
            stub_url: format!("http://{server}"),
            token: token.to_owned(),
        },
        authority: Proxy::authorized(server, token),
        repo: env.dir.join("repo"),
        pushed: BTreeMap::new(),
        pulls: Vec::new(),
        events: 0,
        outcomes: Vec::new(),
        tag: "dispatch",
        fork_heads: BTreeSet::new(),
        pin: None,
    };
    driver
        .send(event.name, event.payload, event.git_ref, event.sha)
        .await
}
