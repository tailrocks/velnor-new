// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::{Mutex, PoisonError};

use velnor_ci::error::CiError;
use velnor_ci::io::{Body, Request, Response, Transport};

#[derive(Debug, Clone)]
pub(super) struct Seen {
    pub(super) method: String,
    pub(super) url: String,
    pub(super) authorization: Option<String>,
    pub(super) body: String,
}

pub(super) struct Reply {
    pub(super) status: u16,
    pub(super) headers: Vec<(&'static str, String)>,
    pub(super) body: Vec<u8>,
}

impl Reply {
    pub(super) fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    pub(super) fn bytes(body: &[u8]) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: body.to_vec(),
        }
    }

    pub(super) fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            headers: vec![("location", location.to_owned())],
            body: Vec::new(),
        }
    }
}

type Status = (u16, Vec<(&'static str, String)>);

#[derive(Default)]
pub(super) struct Fake {
    replies: Mutex<BTreeMap<(String, String), Vec<u8>>>,
    statuses: Mutex<BTreeMap<(String, String), Status>>,
    log: Mutex<Vec<Seen>>,
}

impl Fake {
    pub(super) fn with(mut self, method: &str, url: &str, reply: Reply) -> Self {
        let key = (method.to_owned(), url.to_owned());
        self.replies
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone(), reply.body);
        self.statuses
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, (reply.status, reply.headers));
        self
    }

    pub(super) fn seen(&self) -> Vec<Seen> {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Transport for Fake {
    fn send(&self, request: Request<'_>) -> Result<Response, CiError> {
        let body = match &request.body {
            Body::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => String::new(),
        };
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Seen {
                method: request.method.to_owned(),
                url: request.url.clone(),
                authorization: request
                    .headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                    .map(|(_, value)| value.clone()),
                body,
            });
        let key = (request.method.to_owned(), request.url);
        let bytes = self
            .replies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            .cloned();
        let (status, headers) = self
            .statuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            .cloned()
            .unwrap_or((599, Vec::new()));
        Ok(Response {
            status,
            headers: headers
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
            body: Box::new(Cursor::new(bytes.unwrap_or_default())),
        })
    }
}
