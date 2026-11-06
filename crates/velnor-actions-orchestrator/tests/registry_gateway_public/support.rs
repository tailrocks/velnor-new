// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod layout;
mod request_handlers;
mod token_gate;
pub(super) use layout::Layout;
pub(super) use token_gate::TokenGate;

use std::{
    collections::BTreeMap,
    io::{self, Cursor, Read},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::{AUTHORIZATION, TOKEN};
use sha2::{Digest, Sha256};
use velnor_actions_mise::CheckDeadline;
use velnor_actions_orchestrator::registry_gateway::{
    RegistryIo, RegistryRequest, RegistryResponse,
};

#[derive(Default)]
pub(super) struct Registry {
    pub(super) state: Mutex<State>,
    pub(super) upload_location: Option<String>,
    pub(super) behavior: RegistryBehavior,
    pub(super) active_requests: Arc<AtomicUsize>,
    pub(super) token_gate: Arc<TokenGate>,
    pub(super) token_deadline: Option<CheckDeadline>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum RegistryBehavior {
    #[default]
    Standard,
    TokenRedirect,
    OversizedToken,
    Challenge,
    ManifestRedirect,
    WriteRedirect,
    CdnRead,
    ForeignCdn,
    SecondCdnRedirect,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) blobs: BTreeMap<String, Vec<u8>>,
    pub(super) manifests: BTreeMap<String, Vec<u8>>,
    pub(super) tags: BTreeMap<String, String>,
    pub(super) uploads: BTreeMap<String, Vec<u8>>,
    pub(super) token_requests: usize,
    pub(super) next_upload: usize,
    pub(super) bad_requests: Vec<String>,
}

struct ActiveRequest(Arc<AtomicUsize>);

impl ActiveRequest {
    fn new(active: Arc<AtomicUsize>) -> Self {
        active.fetch_add(1, Ordering::AcqRel);
        Self(active)
    }
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl RegistryIo for Registry {
    fn request(
        &self,
        request: RegistryRequest,
        body: &mut dyn Read,
    ) -> io::Result<RegistryResponse> {
        let _active = ActiveRequest::new(Arc::clone(&self.active_requests));
        let mut bytes = Vec::new();
        body.read_to_end(&mut bytes)?;
        let mut state = self.state.lock().map_err(lock_error)?;
        if request.url.starts_with("https://ghcr.io/token?") {
            return request_handlers::token(self, &request, &mut state);
        }
        if request.url.starts_with("https://trap.invalid/") {
            state.bad_requests.push(request.url);
            return Ok(response(200, Vec::new(), Vec::new()));
        }
        if request.url.starts_with("https://127.0.0.1:9/") {
            state.bad_requests.push(request.url);
            return Ok(response(200, Vec::new(), Vec::new()));
        }
        if request
            .url
            .starts_with("https://pkg-containers.githubusercontent.com/")
        {
            return Ok(request_handlers::cdn(self, &request, &mut state));
        }
        request_handlers::registry(self, &request, bytes, &mut state)
    }
}

fn response(status: u16, mut headers: Vec<(String, String)>, body: Vec<u8>) -> RegistryResponse {
    headers.push(("content-length".into(), body.len().to_string()));
    RegistryResponse {
        status,
        headers,
        body: Box::new(Cursor::new(body)),
    }
}

fn registry_missing() -> RegistryResponse {
    response(
        404,
        vec![("content-type".into(), "application/json".into())],
        br#"{"errors":[{"code":"MANIFEST_UNKNOWN","message":"manifest unknown"}]}"#.to_vec(),
    )
}

fn header<'a>(request: &'a RegistryRequest, name: &str) -> Option<&'a String> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

fn digest_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let encoded = digest
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 0x0f)]),
            ]
        })
        .collect::<String>();
    format!("sha256:{encoded}")
}

fn invalid_request() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "registry fixture request is invalid",
    )
}

pub(super) fn lock_error<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("registry fixture lock is poisoned")
}
