// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use http_body_util::BodyExt;
use hyper::header::UPGRADE;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo};

use super::Body;
use crate::protocol::Error;

#[derive(Debug)]
pub(super) struct Proxy {
    upstream: String,
    client: Client<HttpConnector, Body>,
}

impl Proxy {
    pub(super) fn new(upstream: &str) -> Self {
        Self {
            upstream: upstream.to_owned(),
            client: Client::builder(TokioExecutor::new()).build(HttpConnector::new()),
        }
    }

    pub(super) async fn forward(
        &self,
        mut request: Request<Body>,
    ) -> Result<Response<Body>, Error> {
        let path = request
            .uri()
            .path_and_query()
            .map_or("/", |path| path.as_str());
        *request.uri_mut() = format!("http://{}{path}", self.upstream)
            .parse::<Uri>()
            .map_err(Error::internal)?;
        let downstream = request
            .headers()
            .contains_key(UPGRADE)
            .then(|| hyper::upgrade::on(&mut request));
        let mut response = self
            .client
            .request(request)
            .await
            .map_err(|e| Error::upstream(format!("runner.server: {e}")))?;
        if let (StatusCode::SWITCHING_PROTOCOLS, Some(downstream)) = (response.status(), downstream)
        {
            let upstream = hyper::upgrade::on(&mut response);
            tokio::spawn(async move {
                if let (Ok(downstream), Ok(upstream)) = tokio::join!(downstream, upstream) {
                    let _ = tokio::io::copy_bidirectional(
                        &mut TokioIo::new(downstream),
                        &mut TokioIo::new(upstream),
                    )
                    .await;
                }
            });
        }
        Ok(response.map(|body| body.map_err(std::io::Error::other).boxed_unsync()))
    }
}
