// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::{CONTENT_TYPE, HeaderValue, LINK};
use hyper::{Response, StatusCode};
use serde_json::{Value, json};

use super::Body;
use super::git::Failure;
use crate::rest;

#[derive(Debug)]
pub(super) struct Reply {
    pub(super) status: u16,
    pub(super) body: Option<Value>,
    pub(super) link: Option<String>,
}

impl From<Failure> for Reply {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Missing => Self::missing(),
            Failure::TooLarge => Self::too_large(),
        }
    }
}

impl Reply {
    pub(super) fn ok(body: Value) -> Self {
        Self::with(200, body, None)
    }

    pub(super) fn with(status: u16, body: Value, link: Option<String>) -> Self {
        Self {
            status,
            body: Some(body),
            link,
        }
    }

    pub(super) fn too_large() -> Self {
        Self::with(
            422,
            json!({"message": "Validation Failed", "errors": [{"resource": "Diff", "code": "too_large"}], "status": "422"}),
            None,
        )
    }

    pub(super) fn unreadable_pulls() -> Self {
        Self::with(500, json!({"message": "pulls.json is unreadable"}), None)
    }

    pub(super) fn missing() -> Self {
        Self::with(404, rest::not_found(), None)
    }
}

pub(super) fn text_response(text: String) -> Response<Body> {
    let mut response = Response::new(
        Full::new(Bytes::from(text))
            .map_err(|never| match never {})
            .boxed_unsync(),
    );
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

pub(super) fn response(reply: Reply) -> Response<Body> {
    let bytes = reply
        .body
        .map_or_else(Bytes::new, |body| Bytes::from(body.to_string()));
    let has_body = !bytes.is_empty();
    let mut response = Response::new(
        Full::new(bytes)
            .map_err(|never| match never {})
            .boxed_unsync(),
    );
    *response.status_mut() =
        StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if has_body {
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
    }
    if let Some(link) = reply
        .link
        .and_then(|link| HeaderValue::from_str(&link).ok())
    {
        response.headers_mut().insert(LINK, link);
    }
    response
}
