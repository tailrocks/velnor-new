// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use velnor_local::io::api_stub::{ApiStub, ApiStubConfig, StandIn};
use velnor_test_support::ScratchDir;

use super::{Bound, Reply, job_token_key, temp_dir};

#[derive(Debug)]
pub(crate) struct Stub {
    api: ApiStub,
    control: bool,
    kept: Vec<ScratchDir>,
}

impl Stub {
    pub(crate) fn new(repo: &Path, dir: &Path, cache_dir: &Path) -> Self {
        Self::build(repo, dir, cache_dir, false)
    }

    /// A stub started with `--job-cache-admin`: the override that grants every job.
    pub(crate) fn job_cache_admin(repo: &Path, dir: &Path, cache_dir: &Path) -> Self {
        Self::granting(repo, dir, cache_dir, true, None, (None, None))
    }

    /// A stub started with `--token-key`, `--standin` and `--ledger`.
    pub(crate) fn job_grants(repo: &Path, dir: &Path, cache_dir: &Path, bound: Bound) -> Self {
        Self::granting(repo, dir, cache_dir, false, Some(job_token_key()), bound)
    }

    fn granting(
        repo: &Path,
        dir: &Path,
        cache_dir: &Path,
        job_cache_admin: bool,
        token_key: Option<Vec<u8>>,
        (standin, ledger): Bound,
    ) -> Self {
        Self {
            control: false,
            kept: Vec::new(),
            api: ApiStub::new(ApiStubConfig {
                repo: repo.to_owned(),
                dir: dir.to_owned(),
                cache_dir: cache_dir.to_owned(),
                default_branch: "main".to_owned(),
                diff_limit_bytes: 1 << 20,
                standin,
                local_setup: false,
                job_cache_admin,
                token_key,
                ledger,
            }),
        }
    }

    pub(crate) fn local_setup(repo: &Path, dir: &Path, cache_dir: &Path) -> Self {
        Self::build(repo, dir, cache_dir, true)
    }

    fn build(repo: &Path, dir: &Path, cache_dir: &Path, local_setup: bool) -> Self {
        Self {
            control: false,
            kept: Vec::new(),
            api: ApiStub::new(ApiStubConfig {
                repo: repo.to_owned(),
                dir: dir.to_owned(),
                cache_dir: cache_dir.to_owned(),
                default_branch: "main".to_owned(),
                diff_limit_bytes: 1 << 20,
                standin: None,
                local_setup,
                job_cache_admin: false,
                token_key: None,
                ledger: None,
            }),
        }
    }

    pub(crate) fn over_standin(standin: Arc<dyn StandIn>) -> Self {
        let dir = temp_dir("standin");
        let stub = ApiStub::new(ApiStubConfig {
            repo: dir.to_path_buf(),
            dir: dir.to_path_buf(),
            cache_dir: dir.to_path_buf(),
            default_branch: "main".to_owned(),
            diff_limit_bytes: 1 << 20,
            standin: Some(standin),
            local_setup: false,
            job_cache_admin: false,
            token_key: None,
            ledger: None,
        });
        Self {
            control: false,
            kept: vec![dir],
            api: stub,
        }
    }

    pub(crate) fn keeping(mut self, dirs: Vec<ScratchDir>) -> Self {
        self.kept.extend(dirs);
        self
    }

    pub(crate) fn control(mut self) -> Self {
        self.control = true;
        self
    }

    pub(crate) async fn call(&self, method: &str, path_and_query: &str) -> Reply {
        self.call_with(method, path_and_query, None).await
    }

    /// A call that carries `bearer` as the `Authorization` token, as a job's helper does.
    pub(crate) async fn call_as(&self, method: &str, path_and_query: &str, bearer: &str) -> Reply {
        self.call_with(method, path_and_query, Some(bearer)).await
    }

    async fn call_with(&self, method: &str, path_and_query: &str, bearer: Option<&str>) -> Reply {
        let mut request = Request::builder()
            .method(method)
            .uri(path_and_query)
            .header("host", "api.localhost");
        if self.control {
            request = request.header("authorization", "Bearer test-control");
        } else if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
        }
        let request = request
            .body(
                Full::new(Bytes::new())
                    .map_err(|never| match never {})
                    .boxed_unsync(),
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        let response = if self.control {
            self.api.handle_control("test-control", request).await
        } else {
            self.api.handle(request).await
        };
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
