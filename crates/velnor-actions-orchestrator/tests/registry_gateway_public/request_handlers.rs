// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;

use super::{AUTHORIZATION, TOKEN};
use super::{
    Registry, RegistryBehavior, RegistryRequest, RegistryResponse, State, digest_bytes, header,
    invalid_request, registry_missing, response,
};
use serde_json::json;

pub(super) fn token(
    registry: &Registry,
    request: &RegistryRequest,
    state: &mut State,
) -> io::Result<RegistryResponse> {
    state.token_requests += 1;
    if let Some(deadline) = registry.token_deadline {
        registry.token_gate.wait_until(deadline)?;
    }
    let scope = "scope=repository%3Atailrocks/velnor-job%3Apull%2Cpush";
    if !request.url.contains(scope)
        || header(request, "authorization")
            .is_none_or(|value| value != &format!("Basic {AUTHORIZATION}"))
    {
        state.bad_requests.push(request.url.clone());
        return Ok(response(403, Vec::new(), Vec::new()));
    }
    match registry.behavior {
        RegistryBehavior::TokenRedirect => Ok(response(
            302,
            vec![("location".into(), "https://trap.invalid/token".into())],
            Vec::new(),
        )),
        RegistryBehavior::OversizedToken => Ok(response(200, Vec::new(), vec![b'x'; 65_537])),
        _ => Ok(response(
            200,
            Vec::new(),
            serde_json::to_vec(&json!({"token":TOKEN}))?,
        )),
    }
}

pub(super) fn cdn(
    registry: &Registry,
    request: &RegistryRequest,
    state: &mut State,
) -> RegistryResponse {
    if header(request, "authorization").is_some() {
        state.bad_requests.push(request.url.clone());
        return response(403, Vec::new(), Vec::new());
    }
    if registry.behavior == RegistryBehavior::SecondCdnRedirect {
        return response(
            302,
            vec![("location".into(), "https://trap.invalid/second".into())],
            Vec::new(),
        );
    }
    let Some(digest) = request.url.rsplit("/blobs/").next() else {
        state.bad_requests.push(request.url.clone());
        return response(404, Vec::new(), Vec::new());
    };
    match state.blobs.get(digest) {
        Some(bytes) => response(
            200,
            vec![("content-type".into(), "application/octet-stream".into())],
            bytes.clone(),
        ),
        None => response(404, Vec::new(), Vec::new()),
    }
}

pub(super) fn registry(
    registry: &Registry,
    request: &RegistryRequest,
    bytes: Vec<u8>,
    state: &mut State,
) -> io::Result<RegistryResponse> {
    let Some((_, route)) = request.url.split_once("/v2/tailrocks/velnor-job/") else {
        state.bad_requests.push(request.url.clone());
        return Ok(response(404, Vec::new(), Vec::new()));
    };
    if header(request, "authorization") != Some(&format!("Bearer {TOKEN}")) {
        state.bad_requests.push(request.url.clone());
        return Ok(response(401, Vec::new(), Vec::new()));
    }
    let (path, query) = route.split_once('?').unwrap_or((route, ""));
    if registry.behavior == RegistryBehavior::Challenge && path.starts_with("manifests/") {
        return Ok(response(
            401,
            vec![(
                "www-authenticate".into(),
                "Bearer realm=\"https://trap.invalid/token\"".into(),
            )],
            Vec::new(),
        ));
    }
    if is_upload_start(path) && request.method == "POST" {
        return Ok(start_upload(registry, bytes, state));
    }
    if let Some(upload) = upload_id(path) {
        return continue_upload(request, upload, query, bytes, state);
    }
    if let Some(digest) = path.strip_prefix("blobs/") {
        return Ok(blob(registry, request, digest, state));
    }
    if let Some(reference) = path.strip_prefix("manifests/") {
        return Ok(manifest(registry, request, reference, bytes, state));
    }
    state.bad_requests.push(request.url.clone());
    Ok(response(404, Vec::new(), Vec::new()))
}

fn is_upload_start(path: &str) -> bool {
    matches!(path, "blobs/uploads" | "blobs/uploads/" | "blobs/upload/")
}

fn upload_id(path: &str) -> Option<&str> {
    path.strip_prefix("blobs/uploads/")
        .or_else(|| path.strip_prefix("blobs/upload/"))
}

fn start_upload(registry: &Registry, bytes: Vec<u8>, state: &mut State) -> RegistryResponse {
    state.next_upload += 1;
    let id = format!("upload-{}", state.next_upload);
    state.uploads.insert(id.clone(), bytes);
    let location = registry.upload_location.as_ref().map_or_else(
        || format!("{id}?state=opaque%2Btoken"),
        |value| value.replace("{upload}", &id),
    );
    response(202, vec![("location".into(), location)], Vec::new())
}

fn continue_upload(
    request: &RegistryRequest,
    upload: &str,
    query: &str,
    bytes: Vec<u8>,
    state: &mut State,
) -> io::Result<RegistryResponse> {
    let id = upload.split('?').next().unwrap_or(upload);
    match request.method.as_str() {
        "PATCH" => {
            state.uploads.entry(id.into()).or_default().extend(bytes);
            Ok(response(
                202,
                vec![("location".into(), format!("{id}?state=opaque%2Btoken"))],
                Vec::new(),
            ))
        }
        "PUT" => {
            let digest = query_value(query, "digest")
                .ok_or_else(invalid_request)?
                .replace("%3A", ":")
                .replace("%3a", ":");
            let data = state.uploads.remove(id).ok_or_else(invalid_request)?;
            if digest_bytes(&data) != digest {
                return Ok(response(400, Vec::new(), Vec::new()));
            }
            state.blobs.insert(digest.clone(), data);
            Ok(response(
                201,
                vec![
                    ("docker-content-digest".into(), digest.clone()),
                    (
                        "location".into(),
                        format!("/v2/tailrocks/velnor-job/blobs/{digest}"),
                    ),
                ],
                Vec::new(),
            ))
        }
        _ => Ok(response(404, Vec::new(), Vec::new())),
    }
}

fn blob(
    registry: &Registry,
    request: &RegistryRequest,
    digest: &str,
    state: &State,
) -> RegistryResponse {
    let digest = normalize_digest(digest);
    match request.method.as_str() {
        "HEAD" if state.blobs.contains_key(&digest) => response(
            200,
            vec![("content-type".into(), "application/octet-stream".into())],
            Vec::new(),
        ),
        "GET"
            if matches!(
                registry.behavior,
                RegistryBehavior::CdnRead
                    | RegistryBehavior::ForeignCdn
                    | RegistryBehavior::SecondCdnRedirect
            ) =>
        {
            let host = if registry.behavior == RegistryBehavior::ForeignCdn {
                "trap.invalid"
            } else {
                "pkg-containers.githubusercontent.com"
            };
            response(
                302,
                vec![(
                    "location".into(),
                    format!("https://{host}/ghcr1/blobs/{digest}"),
                )],
                Vec::new(),
            )
        }
        "GET" => match state.blobs.get(&digest) {
            Some(bytes) => response(
                200,
                vec![("content-type".into(), "application/octet-stream".into())],
                bytes.clone(),
            ),
            None => response(404, Vec::new(), Vec::new()),
        },
        _ => response(404, Vec::new(), Vec::new()),
    }
}

fn manifest(
    registry: &Registry,
    request: &RegistryRequest,
    reference: &str,
    bytes: Vec<u8>,
    state: &mut State,
) -> RegistryResponse {
    if request.method == "PUT" {
        if registry.behavior == RegistryBehavior::WriteRedirect {
            return response(
                307,
                vec![("location".into(), "https://trap.invalid/write".into())],
                Vec::new(),
            );
        }
        let digest = digest_bytes(&bytes);
        state.manifests.insert(digest.clone(), bytes);
        state.tags.insert(reference.into(), digest.clone());
        return response(
            201,
            vec![("docker-content-digest".into(), digest)],
            Vec::new(),
        );
    }
    if registry.behavior == RegistryBehavior::ManifestRedirect {
        return response(
            302,
            vec![("location".into(), "https://trap.invalid/manifest".into())],
            Vec::new(),
        );
    }
    let digest = state
        .tags
        .get(reference)
        .cloned()
        .unwrap_or_else(|| reference.into());
    match state.manifests.get(&digest) {
        Some(manifest) => response(
            200,
            vec![
                (
                    "content-type".into(),
                    "application/vnd.oci.image.manifest.v1+json".into(),
                ),
                ("docker-content-digest".into(), digest),
            ],
            manifest.clone(),
        ),
        None => registry_missing(),
    }
}

fn query_value<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        pair.split_once('=')
            .filter(|(key, _)| *key == name)
            .map(|(_, value)| value)
    })
}

fn normalize_digest(digest: &str) -> String {
    digest.strip_prefix("sha256:").map_or_else(
        || format!("sha256:{digest}"),
        |suffix| format!("sha256:{suffix}"),
    )
}
