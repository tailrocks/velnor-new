// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::sync::{Mutex, MutexGuard, PoisonError};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use velnor_ci::error::CiError;
use velnor_ci::image_candidate::hashing::sha256_hex;
use velnor_ci::io::{Body, Request, Response, Transport};

pub(super) const TOKEN: &str = "secret-token-value-0123456789";
pub(super) const ACTOR: &str = "octocat";

pub(super) const REGISTRY_TOKEN: &str = "registry-token-0123456789";
const OBJECTS: &str = "/v2/tailrocks/velnor-job/";
const DOWNLOAD: &str = "https://pkg-containers.githubusercontent.com/";

#[derive(Clone, Debug)]
pub(super) struct Seen {
    pub(super) method: String,
    pub(super) url: String,
    pub(super) authorization: Option<String>,
}

#[derive(Default)]
pub(super) struct Flaws {
    pub(super) corrupt_blob: bool,
    pub(super) redirect: Option<String>,
    pub(super) drop_blob_put: bool,
    pub(super) challenge: Option<String>,
}

#[derive(Default)]
struct State {
    blobs: BTreeMap<String, Vec<u8>>,
    manifests: BTreeMap<String, Vec<u8>>,
    log: Vec<Seen>,
}

#[derive(Default)]
pub(super) struct Fake {
    state: Mutex<State>,
    flaws: Flaws,
}

fn reply(status: u16, headers: &[(&str, String)], body: Vec<u8>) -> Response {
    Response {
        status,
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
        body: Box::new(Cursor::new(body)),
    }
}

fn found(digest: &str, body: &[u8]) -> Response {
    reply(
        200,
        &[
            ("docker-content-digest", digest.to_owned()),
            ("content-length", body.len().to_string()),
        ],
        body.to_vec(),
    )
}

impl Fake {
    pub(super) fn with(flaws: Flaws) -> Self {
        Self {
            state: Mutex::default(),
            flaws,
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn seen(&self) -> Vec<Seen> {
        self.state().log.clone()
    }

    pub(super) fn blobs(&self) -> Vec<String> {
        self.state().blobs.keys().cloned().collect()
    }

    pub(super) fn manifests(&self) -> Vec<String> {
        self.state().manifests.keys().cloned().collect()
    }

    fn route(&self, request: &Seen, payload: &[u8]) -> Result<Response, CiError> {
        let rest = request
            .url
            .strip_prefix("https://ghcr.io")
            .unwrap_or_default();
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let basic = format!("Basic {}", STANDARD.encode(format!("{ACTOR}:{TOKEN}")));
        match (request.method.as_str(), path) {
            ("GET", "/v2/") => {
                let challenge = self.flaws.challenge.clone().unwrap_or_else(|| {
                    r#"Bearer realm="https://ghcr.io/token",service="ghcr.io""#.to_owned()
                });
                return Ok(reply(401, &[("www-authenticate", challenge)], vec![]));
            }
            ("GET", "/token") if request.authorization.as_deref() == Some(basic.as_str()) => {
                let body = format!(r#"{{"token":"{REGISTRY_TOKEN}"}}"#);
                return Ok(reply(200, &[], body.into_bytes()));
            }
            _ => {}
        }
        if request.authorization.as_deref() != Some(&format!("Bearer {REGISTRY_TOKEN}")) {
            return Ok(reply(401, &[], vec![]));
        }
        self.object(request, path, query, payload)
    }

    fn object(
        &self,
        request: &Seen,
        path: &str,
        query: &str,
        payload: &[u8],
    ) -> Result<Response, CiError> {
        let Some(tail) = path.strip_prefix(OBJECTS) else {
            return Ok(reply(404, &[], vec![]));
        };
        let mut state = self.state();
        match (request.method.as_str(), tail.split_once('/')) {
            ("POST", Some(("blobs", "uploads/"))) => Ok(reply(
                202,
                &[("location", format!("{OBJECTS}blobs/uploads/u1?_state=abc"))],
                vec![],
            )),
            ("PUT", Some(("blobs", _))) => {
                let digest = query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("digest="))
                    .unwrap_or_default()
                    .replace("%3A", ":");
                if digest != format!("sha256:{}", sha256_hex(payload)) {
                    return Ok(reply(400, &[], vec![]));
                }
                state.blobs.insert(digest, payload.to_vec());
                if self.flaws.drop_blob_put {
                    return Err(CiError::new("connection dropped"));
                }
                Ok(reply(201, &[], vec![]))
            }
            ("PUT", Some(("manifests", digest))) => {
                if digest != format!("sha256:{}", sha256_hex(payload)) {
                    return Ok(reply(400, &[], vec![]));
                }
                state.manifests.insert(digest.to_owned(), payload.to_vec());
                Ok(reply(
                    201,
                    &[("docker-content-digest", digest.to_owned())],
                    vec![],
                ))
            }
            (method, Some((kind @ ("blobs" | "manifests"), digest))) => {
                let store = if kind == "blobs" {
                    &state.blobs
                } else {
                    &state.manifests
                };
                let Some(body) = store.get(digest) else {
                    return Ok(reply(404, &[], vec![]));
                };
                if method == "HEAD" {
                    let mut head = found(digest, body);
                    head.body = Box::new(Cursor::new(vec![]));
                    return Ok(head);
                }
                if kind == "blobs"
                    && let Some(target) = &self.flaws.redirect
                {
                    let location = target.replace("{digest}", digest);
                    return Ok(reply(307, &[("location", location)], vec![]));
                }
                let mut served = body.clone();
                if self.flaws.corrupt_blob && kind == "blobs" {
                    served[0] ^= 1;
                }
                Ok(found(digest, &served))
            }
            _ => Ok(reply(404, &[], vec![])),
        }
    }

    fn download(&self, request: &Seen) -> Response {
        let state = self.state();
        let name = request.url.rsplit('/').next().unwrap_or_default();
        let digest = name.split('?').next().unwrap_or_default();
        state
            .blobs
            .get(digest)
            .map_or_else(|| reply(404, &[], vec![]), |body| found(digest, body))
    }
}

impl Transport for Fake {
    fn send(&self, request: Request<'_>) -> Result<Response, CiError> {
        let Request {
            method,
            url,
            headers,
            body,
        } = request;
        let payload = match body {
            Body::Empty => Vec::new(),
            Body::Bytes(bytes) => bytes.to_vec(),
            Body::Stream { mut reader, length } => {
                let mut data = Vec::new();
                reader
                    .read_to_end(&mut data)
                    .map_err(|e| CiError::new(e.to_string()))?;
                assert_eq!(
                    data.len() as u64,
                    length,
                    "the stream is as long as it declares"
                );
                data
            }
        };
        let seen = Seen {
            method: method.to_owned(),
            authorization: headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(_, value)| value.clone()),
            url,
        };
        self.state().log.push(seen.clone());
        if seen.url.starts_with(DOWNLOAD) {
            return Ok(self.download(&seen));
        }
        self.route(&seen, &payload)
    }
}
