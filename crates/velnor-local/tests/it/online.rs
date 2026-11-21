// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::future::Future;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use velnor_local::io::authority::Backend;
use velnor_local::io::pair::{MintError, Minted};
use velnor_local::io::supervisor::{await_online, await_replacement};
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
    let message = error.err().map_or_default(|error| error.to_string());
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
    let message = error.err().map_or_default(|error| error.to_string());
    assert!(message.contains("1 of 2 trusted runners"), "{message}");
}

struct Silent;

impl Backend for Silent {
    fn get(&self, _: Side, _: &str) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        std::future::pending()
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

#[tokio::test(start_paused = true)]
async fn it_honours_the_deadline_when_runner_server_never_answers() {
    let result = tokio::time::timeout(
        Duration::from_secs(120),
        await_online(&Silent, &SIDES, Duration::from_secs(60)),
    )
    .await;
    let message = result
        .expect("await_online hung past its own deadline")
        .err()
        .map_or_default(|error| error.to_string());
    assert!(message.contains("did not answer within 60 s"), "{message}");
}

struct Flapping {
    trusted_asked: AtomicUsize,
    untrusted_asked: AtomicUsize,
}

impl Backend for Flapping {
    fn get(
        &self,
        side: Side,
        path: &str,
    ) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        let body = if path.contains("isagentonline") {
            let online = match side {
                Side::Trusted => self.trusted_asked.fetch_add(1, Ordering::SeqCst) == 0,
                Side::Untrusted => self.untrusted_asked.fetch_add(1, Ordering::SeqCst) >= 2,
            };
            format!(r#"{{"online":{online}}}"#)
        } else {
            r#"{"value":[{"name":"a-1"}],"count":1}"#.to_owned()
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

#[tokio::test(start_paused = true)]
async fn it_is_not_ready_when_an_earlier_side_dropped_before_a_later_side_came_up() {
    let server = Flapping {
        trusted_asked: AtomicUsize::new(0),
        untrusted_asked: AtomicUsize::new(0),
    };
    let sides = [(Side::Trusted, 1), (Side::Untrusted, 1)];
    let result = await_online(&server, &sides, Duration::from_secs(5)).await;
    assert!(result.is_err(), "{result:?}");
}

struct FailsOnce {
    listed: AtomicUsize,
}

impl Backend for FailsOnce {
    fn get(&self, _: Side, path: &str) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        let answer = if path.contains("isagentonline") {
            Ok(Some(br#"{"online":true}"#.to_vec()))
        } else if self.listed.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(io::Error::other("connection reset"))
        } else {
            Ok(Some(br#"{"value":[{"name":"a-1"}],"count":1}"#.to_vec()))
        };
        std::future::ready(answer)
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

#[tokio::test(start_paused = true)]
async fn it_retries_a_transient_request_failure_until_the_deadline() {
    let server = FailsOnce {
        listed: AtomicUsize::new(0),
    };
    let sides = [(Side::Trusted, 1)];
    let result = await_online(&server, &sides, Duration::from_secs(5)).await;
    assert!(result.is_ok(), "{result:?}");
    assert!(server.listed.load(Ordering::SeqCst) >= 2);
}

struct Replaced {
    asked: AtomicUsize,
    replaced_after: usize,
}

impl Backend for Replaced {
    fn get(&self, _: Side, path: &str) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        let asked = self.asked.fetch_add(1, Ordering::SeqCst);
        let replaced = asked >= self.replaced_after;
        let body = if path.contains("isagentonline") {
            format!(r#"{{"online":{}}}"#, !path.ends_with("fresh") || replaced)
        } else if replaced {
            r#"{"value":[{"name":"consumed"},{"name":"fresh"}]}"#.to_owned()
        } else {
            r#"{"value":[{"name":"consumed"}]}"#.to_owned()
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

fn consumed() -> BTreeSet<String> {
    BTreeSet::from(["consumed".to_owned()])
}

#[tokio::test]
async fn it_waits_for_the_replacement_pair_while_the_consumed_agent_still_answers_online() {
    let server = Replaced {
        asked: AtomicUsize::new(0),
        replaced_after: 6,
    };
    let result = await_replacement(
        &server,
        Side::Trusted,
        1,
        &consumed(),
        Duration::from_secs(10),
    )
    .await;
    assert!(result.is_ok(), "{result:?}");
    assert!(server.asked.load(Ordering::SeqCst) > 6);
}

#[tokio::test]
async fn it_fails_clearly_when_only_the_consumed_agent_is_online() {
    let server = Replaced {
        asked: AtomicUsize::new(0),
        replaced_after: usize::MAX,
    };
    let error = await_replacement(
        &server,
        Side::Trusted,
        1,
        &consumed(),
        Duration::from_millis(500),
    )
    .await;
    let message = error
        .expect_err("the consumed agent is not a replacement")
        .to_string();
    assert!(
        message.contains("no replacement trusted pair registered"),
        "{message}"
    );
    assert!(message.contains("0 of 1"), "{message}");
}
