// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use velnor_local::io::authority::Backend;
use velnor_local::io::pair::{MintError, Minted};
use velnor_local::io::supervisor::await_online;
use velnor_local::side::Side;

struct Server {
    agents: &'static [&'static str],
    online_after: usize,
    asked: AtomicUsize,
}

impl Backend for Server {
    fn get(&self, _: Side, path: &str) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        let body = if path.contains("isagentonline") {
            let asked = self.asked.fetch_add(1, Ordering::SeqCst);
            format!(r#"{{"online":{}}}"#, asked >= self.online_after)
        } else {
            let value: Vec<String> = self
                .agents
                .iter()
                .map(|name| format!(r#"{{"name":"{name}"}}"#))
                .collect();
            format!(
                r#"{{"value":[{}],"count":{}}}"#,
                value.join(","),
                value.len()
            )
        };
        std::future::ready(Ok(Some(body.into_bytes())))
    }

    fn delete(&self, _: Side, _: &str) -> impl Future<Output = io::Result<()>> + Send {
        std::future::ready(Ok(()))
    }

    fn mint(
        &self,
        _: Side,
        _: Option<&str>,
    ) -> impl Future<Output = Result<Option<Minted>, MintError>> + Send {
        std::future::ready(Ok(None))
    }
}

const SIDES: [(Side, u16); 2] = [(Side::Trusted, 2), (Side::Untrusted, 0)];

#[tokio::test]
async fn it_waits_until_every_pair_runner_is_online_at_runner_server() {
    let server = Server {
        agents: &["a-1", "a-2"],
        online_after: 5,
        asked: AtomicUsize::new(0),
    };
    let result = await_online(&server, &SIDES, Duration::from_secs(10)).await;
    assert!(result.is_ok(), "{result:?}");
    assert!(server.asked.load(Ordering::SeqCst) > 5);
}

#[tokio::test]
async fn it_fails_naming_the_count_when_a_runner_never_comes_online() {
    let server = Server {
        agents: &["a-1", "a-2"],
        online_after: usize::MAX,
        asked: AtomicUsize::new(0),
    };
    let error = await_online(&server, &SIDES, Duration::from_millis(500)).await;
    let message = error
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("0 of 2 trusted runners were online"),
        "{message}"
    );
}

#[tokio::test]
async fn it_needs_as_many_online_agents_as_pairs() {
    let server = Server {
        agents: &["a-1"],
        online_after: 0,
        asked: AtomicUsize::new(0),
    };
    let error = await_online(&server, &SIDES, Duration::from_millis(500)).await;
    let message = error
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(message.contains("1 of 2 trusted runners"), "{message}");
}
