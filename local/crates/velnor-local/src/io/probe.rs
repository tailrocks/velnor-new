// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::Request;

use super::proxy::Proxy;

pub(super) async fn request(
    proxy: &Proxy,
    method: &str,
    path: &str,
    body: &str,
) -> Option<Vec<u8>> {
    let body = Full::new(bytes::Bytes::from(body.to_owned()));
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(body.map_err(|never| match never {}).boxed_unsync())
        .ok()?;
    let response = proxy.forward(request).await.ok()?;
    let ok = response.status().is_success();
    let bytes = response.into_body().collect().await.ok()?.to_bytes();
    ok.then(|| bytes.to_vec())
}

pub(super) async fn wait_for(
    proxy: &Proxy,
    path: &str,
    deadline: tokio::time::Instant,
    what: &str,
) -> io::Result<()> {
    poll(proxy, path, deadline, what, |_| Ok(())).await
}

pub(super) async fn wait_online(
    proxy: &Proxy,
    path: &str,
    deadline: tokio::time::Instant,
    what: &str,
) -> io::Result<()> {
    poll(
        proxy,
        path,
        deadline,
        what,
        |body| match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(answer) if answer["online"] == true => Ok(()),
            Ok(answer) if answer["online"] == false => Err("offline".to_owned()),
            Ok(_) => Err("answer has no boolean `online`".to_owned()),
            Err(error) => Err(format!("malformed answer: {error}")),
        },
    )
    .await
}

async fn poll(
    proxy: &Proxy,
    path: &str,
    deadline: tokio::time::Instant,
    what: &str,
    accept: impl Fn(&[u8]) -> Result<(), String>,
) -> io::Result<()> {
    let mut last = "no answer".to_owned();
    loop {
        if let Ok(Some(body)) =
            tokio::time::timeout_at(deadline, request(proxy, "GET", path, "")).await
        {
            match accept(&body) {
                Ok(()) => return Ok(()),
                Err(reason) => last = reason,
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "{what} did not connect before the readiness deadline (last state: {last})"
            )));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
