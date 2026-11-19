// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use super::deregister_and_confirm;
use crate::scale_set::EnsureError;

/// Replays canned replies in order and records each request; running out is a reset.
struct Script {
    replies: VecDeque<Result<(u16, String), TransportFail>>,
    seen: Vec<Method>,
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.method);
        let (status, body) = self.replies.pop_front().ok_or(TransportFail::Reset)??;
        Ok(Exchange {
            status,
            body: body.into_bytes(),
        })
    }
}

fn script(replies: Vec<Result<(u16, String), TransportFail>>) -> Script {
    Script {
        replies: replies.into(),
        seen: Vec::new(),
    }
}

fn page(ids: &[i64]) -> String {
    let value: Vec<String> = ids
        .iter()
        .map(|id| format!(r#"{{"id":{id},"name":"runner-{id}","runnerScaleSetId":10}}"#))
        .collect();
    format!(r#"{{"count":{},"value":[{}]}}"#, ids.len(), value.join(","))
}

#[test]
fn deleted_and_no_longer_listed_is_confirmed() {
    let mut transport = script(vec![Ok((204, String::new())), Ok((200, page(&[])))]);
    assert_eq!(deregister_and_confirm(&mut transport, "t", 10, 77), Ok(()));
    assert_eq!(transport.seen, [Method::Delete, Method::Get]);
}

#[test]
fn a_rejected_deletion_with_the_runner_still_listed_is_not_confirmed() {
    for delete in [
        Ok((500, String::new())),
        Ok((404, String::new())),
        Err(TransportFail::Reset),
    ] {
        let mut transport = script(vec![delete, Ok((200, page(&[77])))]);
        assert_eq!(
            deregister_and_confirm(&mut transport, "t", 10, 77),
            Err(EnsureError::Uncertain)
        );
    }
}

#[test]
fn a_listing_that_cannot_be_read_is_not_confirmed() {
    for listing in [
        Ok((500, String::new())),
        Ok((200, "not json".to_owned())),
        Ok((200, r#"{"count":2,"value":[]}"#.to_owned())),
        Err(TransportFail::Reset),
    ] {
        let mut transport = script(vec![Ok((204, String::new())), listing]);
        assert!(deregister_and_confirm(&mut transport, "t", 10, 77).is_err());
    }
}

#[test]
fn an_already_absent_runner_is_confirmed_by_the_listing_alone() {
    let mut transport = script(vec![Ok((404, String::new())), Ok((200, page(&[78])))]);
    assert_eq!(deregister_and_confirm(&mut transport, "t", 10, 77), Ok(()));
}
