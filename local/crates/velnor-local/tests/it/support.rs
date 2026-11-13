// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::HeaderMap;
use hyper::{Request, Uri};
use serde_json::{Value, json};
use velnor_local::io::api_stub::{ApiStub, ApiStubConfig};
use velnor_local::io::{Body, CacheServer, Config};

pub(crate) const MAIN: &str = "refs/heads/main";
pub(crate) const PULL_1: &str = "refs/pull/1/merge";
pub(crate) const PULL_2: &str = "refs/pull/2/merge";

#[derive(Debug)]
pub(crate) struct Reply {
    pub(crate) status: u16,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
}

impl Reply {
    pub(crate) fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| unreachable!("not JSON: {e}"))
    }
}

#[derive(Debug)]
pub(crate) struct Harness {
    dir: PathBuf,
    limit_bytes: u64,
    server: Arc<CacheServer>,
}

impl Harness {
    pub(crate) async fn new(name: &str, limit_bytes: u64) -> Self {
        let dir =
            std::env::temp_dir().join(format!("velnor-local-it-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self::open(dir, limit_bytes).await
    }

    async fn open(dir: PathBuf, limit_bytes: u64) -> Self {
        let config = Config {
            dir: dir.clone(),
            token_key: STANDARD
                .decode(fixture()["public_key"].as_str().unwrap_or_default())
                .unwrap_or_else(|e| unreachable!("{e}")),
            upstream: "runner-server.invalid:80".to_owned(),
            limit_bytes,
        };
        let server = CacheServer::open(config)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        Self {
            dir,
            limit_bytes,
            server,
        }
    }

    pub(crate) async fn reopen(self) -> Self {
        Self::open(self.dir, self.limit_bytes).await
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) async fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Bytes,
    ) -> Reply {
        let uri: Uri = url.parse().unwrap_or_else(|e| unreachable!("{e}"));
        let mut request = Request::builder()
            .method(method)
            .uri(uri.path_and_query().map_or("/", |path| path.as_str()));
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let body: Body = Full::new(body)
            .map_err(|never| match never {})
            .boxed_unsync();
        let response = self
            .server
            .handle(request.body(body).unwrap_or_else(|e| unreachable!("{e}")))
            .await;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
            .to_bytes();
        Reply {
            status,
            headers,
            body,
        }
    }

    pub(crate) async fn twirp(&self, git_ref: &str, call: &str, body: &Value) -> Reply {
        self.twirp_with(&token(git_ref), call, body).await
    }

    pub(crate) async fn twirp_with(&self, token: &str, call: &str, body: &Value) -> Reply {
        let url = format!(
            "http://github.localhost/twirp/github.actions.results.api.v1.CacheService/{call}"
        );
        let headers = [
            ("content-type", "application/json".to_owned()),
            ("host", "github.localhost".to_owned()),
            ("authorization", format!("Bearer {token}")),
        ];
        self.send("POST", &url, &headers, Bytes::from(body.to_string()))
            .await
    }

    pub(crate) async fn restore(&self, git_ref: &str, key: &str) -> Value {
        self.twirp(
            git_ref,
            "GetCacheEntryDownloadURL",
            &json!({"key": key, "version": "v"}),
        )
        .await
        .json()
    }

    pub(crate) async fn put(&self, url: &str, body: &[u8]) -> Reply {
        let headers = [("x-ms-blob-type", "BlockBlob".to_owned())];
        self.send("PUT", url, &headers, Bytes::copy_from_slice(body))
            .await
    }

    pub(crate) async fn get(&self, url: &str) -> Reply {
        self.send("GET", url, &[], Bytes::new()).await
    }

    pub(crate) async fn register(&self, number: u64) {
        let url = format!("http://github.localhost/_velnor/pull-requests/{number}");
        let reply = self
            .send("PUT", &url, &[], Bytes::from(r#"{"base_branch":"main"}"#))
            .await;
        assert_eq!(reply.status, 200);
    }

    pub(crate) async fn save(&self, git_ref: &str, key: &str, archive: &[u8]) -> Value {
        let entry = json!({"key": key, "version": "v"});
        let created = self.twirp(git_ref, "CreateCacheEntry", &entry).await.json();
        let Some(url) = created["signed_upload_url"].as_str() else {
            return created;
        };
        assert_eq!(self.put(url, archive).await.status, 201);
        let finalize = json!({"key": key, "version": "v", "size_bytes": archive.len().to_string()});
        self.twirp(git_ref, "FinalizeCacheEntryUpload", &finalize)
            .await
            .json()
    }
}

#[derive(Debug)]
pub(crate) struct Stub {
    stub: ApiStub,
}

impl Stub {
    pub(crate) fn new(repo: &Path, dir: &Path, cache_dir: &Path) -> Self {
        Self {
            stub: ApiStub::new(ApiStubConfig {
                repo: repo.to_owned(),
                dir: dir.to_owned(),
                cache_dir: cache_dir.to_owned(),
            }),
        }
    }

    pub(crate) async fn call(&self, method: &str, path_and_query: &str) -> Reply {
        let request = Request::builder()
            .method(method)
            .uri(path_and_query)
            .header("host", "api.localhost")
            .body(
                Full::new(Bytes::new())
                    .map_err(|never| match never {})
                    .boxed_unsync(),
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        let response = self.stub.handle(request).await;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
            .to_bytes();
        Reply {
            status,
            headers,
            body,
        }
    }

    pub(crate) async fn get(&self, path_and_query: &str) -> Reply {
        self.call("GET", path_and_query).await
    }
}

impl Reply {
    pub(crate) fn link(&self) -> String {
        self.headers
            .get("link")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    }
}

pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("velnor-local-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/tokens.json");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| unreachable!("{e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn token(git_ref: &str) -> String {
    let fixture = fixture();
    let header = fixture["header"].as_str().unwrap_or_default();
    let entry = &fixture["tokens"][git_ref];
    let claims = entry["claims"].as_str().unwrap_or_default();
    let signature = entry["signature"].as_str().unwrap_or_default();
    format!(
        "{}.{}.{signature}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(claims)
    )
}

pub(crate) fn recorded(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/cache-client")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| unreachable!("{e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| unreachable!("{e}"))
}
