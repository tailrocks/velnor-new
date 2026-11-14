// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::Display;

use serde::Deserialize;
use serde_json::{Value, json};

const TWIRP_PREFIX: &str = "/twirp/github.actions.results.api.v1.CacheService/";
const ARCHIVE_PREFIX: &str = "/velnor-cache/archives/";
const PULL_REQUEST_PREFIX: &str = "/_velnor/pull-requests/";

#[must_use]
pub fn control_only(path: &str) -> bool {
    path.starts_with(PULL_REQUEST_PREFIX)
}

#[must_use]
pub fn writes(method: &str, path: &str) -> bool {
    match route(path) {
        Route::Twirp(call) => call != "GetCacheEntryDownloadURL",
        Route::Archive(_) => method != "GET",
        Route::PullRequest(_) => true,
        Route::Upstream => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route<'a> {
    Twirp(&'a str),
    Archive(&'a str),
    PullRequest(&'a str),
    Upstream,
}

#[must_use]
pub fn route(path: &str) -> Route<'_> {
    if let Some(call) = path.strip_prefix(TWIRP_PREFIX) {
        Route::Twirp(call)
    } else if let Some(blob) = path.strip_prefix(ARCHIVE_PREFIX) {
        Route::Archive(blob)
    } else if let Some(number) = path.strip_prefix(PULL_REQUEST_PREFIX) {
        Route::PullRequest(number)
    } else {
        Route::Upstream
    }
}

#[derive(Debug, Deserialize)]
pub struct CacheRequest {
    pub key: String,
    pub version: String,
    #[serde(default)]
    pub restore_keys: Vec<String>,
    #[serde(default)]
    pub size_bytes: String,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestBase {
    pub base_branch: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Upload {
    Whole,
    Block(String),
    BlockList,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub status: u16,
    pub code: &'static str,
    pub msg: String,
}

impl Error {
    fn new(status: u16, code: &'static str, msg: impl Display) -> Self {
        Self {
            status,
            code,
            msg: msg.to_string(),
        }
    }

    pub fn invalid(msg: impl Display) -> Self {
        Self::new(400, "invalid_argument", msg)
    }

    pub fn unauthenticated(msg: impl Display) -> Self {
        Self::new(401, "unauthenticated", msg)
    }

    pub fn not_found(msg: impl Display) -> Self {
        Self::new(404, "not_found", msg)
    }

    pub fn precondition(msg: impl Display) -> Self {
        Self::new(412, "failed_precondition", msg)
    }

    pub fn internal(msg: impl Display) -> Self {
        Self::new(500, "internal", msg)
    }

    pub fn upstream(msg: impl Display) -> Self {
        Self::new(502, "unavailable", msg)
    }

    #[must_use]
    pub fn body(&self) -> Value {
        json!({"code": self.code, "msg": self.msg})
    }
}

pub fn parse<'a, T: Deserialize<'a>>(body: &'a [u8]) -> Result<T, Error> {
    serde_json::from_slice(body).map_err(Error::invalid)
}

#[must_use]
pub fn archive_url(host: &str, blob: &str) -> String {
    format!("http://{host}{ARCHIVE_PREFIX}{blob}")
}

pub fn upload_kind(query: &str) -> Result<Upload, Error> {
    let params: BTreeMap<String, String> = form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    match (
        params.get("comp").map(String::as_str),
        params.get("blockid"),
    ) {
        (None, _) => Ok(Upload::Whole),
        (Some("block"), Some(id)) => Ok(Upload::Block(id.clone())),
        (Some("blocklist"), _) => Ok(Upload::BlockList),
        _ => Err(Error::invalid(format!("unsupported upload query {query}"))),
    }
}

pub fn block_list(xml: &str) -> Result<Vec<String>, Error> {
    let ids: Vec<String> = xml
        .split("<Latest>")
        .skip(1)
        .map(|rest| rest.split_once("</Latest>").map(|(id, _)| id.to_owned()))
        .collect::<Option<_>>()
        .ok_or_else(|| Error::invalid("unterminated <Latest> in the block list"))?;
    if ids.is_empty() {
        return Err(Error::invalid("the block list names no <Latest> block"));
    }
    Ok(ids)
}
