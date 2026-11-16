// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::time::Duration;

use serde_json::Value;

use crate::authority::AGENTS_PATH;
use crate::io::authority::Backend;
use crate::side::Side;

const ONLINE_PATH: &str = "/_apis/v1/Message/isagentonline?name=";
const POLL: Duration = Duration::from_millis(200);

async fn names<B: Backend>(backend: &B, side: Side) -> io::Result<Vec<String>> {
    let listed = backend.get(side, AGENTS_PATH).await?.unwrap_or_default();
    let listed: Value = serde_json::from_slice(&listed).map_err(io::Error::other)?;
    Ok(listed["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|agent| agent["name"].as_str().map(str::to_owned))
        .collect())
}

async fn online<B: Backend>(backend: &B, side: Side, name: &str) -> bool {
    let answer = backend.get(side, &format!("{ONLINE_PATH}{name}")).await;
    let Ok(Some(answer)) = answer else {
        return false;
    };
    serde_json::from_slice::<Value>(&answer).is_ok_and(|answer| answer["online"] == true)
}

async fn ready<B: Backend>(backend: &B, side: Side) -> io::Result<usize> {
    let mut ready = 0;
    for name in &names(backend, side).await? {
        ready += usize::from(online(backend, side, name).await);
    }
    Ok(ready)
}

pub async fn await_online<B: Backend>(
    backend: &B,
    sides: &[(Side, u16)],
    timeout: Duration,
) -> io::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    let sides: Vec<_> = sides.iter().filter(|(_, pairs)| *pairs > 0).collect();
    loop {
        let mut short = None;
        let mut failed = None;
        for &&(side, pairs) in &sides {
            let Ok(ready) = tokio::time::timeout_at(deadline, ready(backend, side)).await else {
                return Err(io::Error::other(format!(
                    "{} runner.server did not answer within {} s",
                    side.name(),
                    timeout.as_secs()
                )));
            };
            let ready = match ready {
                Ok(ready) => ready,
                Err(error) => {
                    failed = Some(error);
                    0
                }
            };
            if ready < usize::from(pairs) && short.is_none() {
                short = Some((side, pairs, ready));
            }
        }
        let Some((side, pairs, ready)) = short else {
            return Ok(());
        };
        if tokio::time::Instant::now() >= deadline {
            let last = failed.map_or_else(String::new, |error| format!(" (last error: {error})"));
            return Err(io::Error::other(format!(
                "{ready} of {pairs} {} runners were online at runner.server within {} s{last}",
                side.name(),
                timeout.as_secs()
            )));
        }
        tokio::time::sleep(POLL).await;
    }
}
