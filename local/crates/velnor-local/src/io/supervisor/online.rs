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

pub async fn await_online<B: Backend>(
    backend: &B,
    sides: &[(Side, u16)],
    timeout: Duration,
) -> io::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    for &(side, pairs) in sides.iter().filter(|(_, pairs)| *pairs > 0) {
        loop {
            let listed = names(backend, side).await?;
            let mut ready = 0;
            for name in &listed {
                ready += usize::from(online(backend, side, name).await);
            }
            if ready >= usize::from(pairs) {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::other(format!(
                    "{ready} of {pairs} {} runners were online at runner.server within {} s",
                    side.name(),
                    timeout.as_secs()
                )));
            }
            tokio::time::sleep(POLL).await;
        }
    }
    Ok(())
}
