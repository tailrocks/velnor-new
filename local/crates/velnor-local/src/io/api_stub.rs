// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::PathBuf;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::{CONTENT_TYPE, HOST, HeaderValue, LINK};
use hyper::{Request, Response, StatusCode};
use serde_json::{Value, json};
use tokio::fs;
use tokio::process::Command;

use super::{Body, now, read_index, write_index};
use crate::github::{
    self, Api, COMMIT_FORMAT, COMPARE_COMMITS_DEFAULT, COMPARE_FILES_CAP, CONTENTS_LIMIT_BYTES,
    Commit, Comparison, DEFAULT_PER_PAGE, FileDiff, PULL_FILES_CAP, Page, Site, TreeEntry,
};

#[derive(Debug)]
pub struct ApiStubConfig {
    pub repo: PathBuf,
    pub dir: PathBuf,
    pub cache_dir: PathBuf,
}

#[derive(Debug)]
pub struct ApiStub {
    config: ApiStubConfig,
}

#[derive(Debug)]
struct Reply {
    status: u16,
    body: Option<Value>,
    link: Option<String>,
}

impl Reply {
    fn ok(body: Value) -> Self {
        Self::with(200, body, None)
    }

    fn with(status: u16, body: Value, link: Option<String>) -> Self {
        Self {
            status,
            body: Some(body),
            link,
        }
    }

    fn missing() -> Self {
        Self::with(404, github::not_found(), None)
    }
}

struct Call<'a> {
    host: &'a str,
    path: &'a str,
    params: BTreeMap<String, String>,
    site: Site<'a>,
}

impl ApiStub {
    #[must_use]
    pub fn new(config: ApiStubConfig) -> Self {
        Self { config }
    }

    pub async fn handle(&self, request: Request<Body>) -> Response<Body> {
        let method = request.method().as_str().to_owned();
        let path = request.uri().path().to_owned();
        let host = request
            .headers()
            .get(HOST)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("localhost")
            .to_owned();
        let repository = github::repository(&path);
        let call = Call {
            host: &host,
            path: &path,
            params: github::query(request.uri().query().unwrap_or_default()),
            site: Site {
                host: &host,
                repository: &repository,
            },
        };
        let reply = match github::route(&method, &path) {
            Api::PullFiles { number } => self.pull_files(&call, number).await,
            Api::PullList => self.pull_list(&call).await,
            Api::Compare { base, head } => self.compare(&call, &base, &head).await,
            Api::Contents { path } => self.contents(&call, &path).await,
            Api::CacheList => self.cache_list(&call).await,
            Api::CacheDelete { id } => self.cache_delete(&call, id).await,
            Api::NotImplemented => Reply::with(501, github::not_implemented(&method, &path), None),
        };
        eprintln!("{method} {path} {}", reply.status);
        response(reply)
    }

    async fn git(&self, args: &[&str]) -> Option<Vec<u8>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.config.repo)
            .args(["-c", "core.quotepath=false"])
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .await
            .ok()?;
        output.status.success().then_some(output.stdout)
    }

    async fn git_text(&self, args: &[&str]) -> Option<String> {
        self.git(args)
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn resolve(&self, reference: &str) -> Option<String> {
        if reference.is_empty() || reference.starts_with('-') {
            return None;
        }
        let spec = format!("{reference}^{{commit}}");
        let sha = self
            .git_text(&["rev-parse", "--verify", "--quiet", &spec])
            .await?;
        Some(sha.trim().to_owned())
    }

    async fn diff(&self, base: &str, head: &str) -> Option<Vec<FileDiff>> {
        let range = format!("{base}...{head}");
        let text = self
            .git_text(&[
                "diff",
                "-M",
                "--full-index",
                "--no-color",
                "--no-ext-diff",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                &range,
            ])
            .await?;
        Some(github::parse_diff(&text))
    }

    async fn commits(&self, args: &[&str]) -> Option<Vec<Commit>> {
        let format = format!("--format={COMMIT_FORMAT}");
        let mut full = vec!["log", "--no-color", &format];
        full.extend_from_slice(args);
        let text = self.git_text(&full).await?;
        Some(github::parse_commits(&text))
    }

    async fn pulls(&self) -> Vec<Value> {
        match fs::read(self.config.dir.join("pulls.json")).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    async fn pull_list(&self, call: &Call<'_>) -> Reply {
        let wanted = call.params.get("state").map_or("open", String::as_str);
        let pulls: Vec<Value> = self
            .pulls()
            .await
            .into_iter()
            .filter(|pull| wanted == "all" || pull["state"].as_str().unwrap_or("open") == wanted)
            .collect();
        paged(call, DEFAULT_PER_PAGE, &pulls)
    }

    async fn pull_files(&self, call: &Call<'_>, number: u64) -> Reply {
        let pulls = self.pulls().await;
        let Some(pull) = pulls.iter().find(|pull| pull["number"] == number) else {
            return Reply::missing();
        };
        let (Some(base), Some(head)) = (pull["base"]["sha"].as_str(), pull["head"]["sha"].as_str())
        else {
            return Reply::missing();
        };
        let Some(files) = self.diff(base, head).await else {
            return Reply::missing();
        };
        let files: Vec<Value> = files
            .iter()
            .take(PULL_FILES_CAP)
            .map(|file| call.site.file(file, head))
            .collect();
        paged(call, DEFAULT_PER_PAGE, &files)
    }

    async fn compare(&self, call: &Call<'_>, base: &str, head: &str) -> Reply {
        let (Some(base), Some(head)) = (self.resolve(base).await, self.resolve(head).await) else {
            return Reply::missing();
        };
        let Some(merge_base) = self
            .git_text(&["merge-base", &base, &head])
            .await
            .map(|sha| sha.trim().to_owned())
        else {
            return Reply::missing();
        };
        let range = format!("{base}..{head}");
        let behind = format!("{head}..{base}");
        let (Some(commits), Some(base_commit), Some(merge_commit), Some(behind), Some(files)) = (
            self.commits(&["--reverse", &range]).await,
            self.commits(&["-1", &base]).await,
            self.commits(&["-1", &merge_base]).await,
            self.git_text(&["rev-list", "--count", &behind]).await,
            self.diff(&base, &head).await,
        ) else {
            return Reply::missing();
        };
        let (Some(base_commit), Some(merge_commit)) = (base_commit.first(), merge_commit.first())
        else {
            return Reply::missing();
        };
        let default = if call.params.contains_key("page") || call.params.contains_key("per_page") {
            DEFAULT_PER_PAGE
        } else {
            COMPARE_COMMITS_DEFAULT
        };
        let page = Page::from_query(&call.params, default);
        let comparison = Comparison {
            base: base_commit,
            head: &head,
            merge_base: merge_commit,
            commits: &commits,
            behind_by: behind.trim().parse().unwrap_or(0),
            files: &files[..files.len().min(COMPARE_FILES_CAP)],
        };
        let body = call.site.compare(&comparison, page, page.slice(&commits));
        let link = page.link(commits.len(), call.host, call.path, &call.params);
        Reply::with(200, body, link)
    }

    async fn contents(&self, call: &Call<'_>, path: &str) -> Reply {
        let reference = call.params.get("ref").map_or("HEAD", String::as_str);
        let Some(commit) = self.resolve(reference).await else {
            return Reply::missing();
        };
        let path = path.trim_end_matches('/');
        let Some(entries) = self.tree(&commit, path).await else {
            return Reply::missing();
        };
        let Some(entry) = entries.iter().find(|entry| entry.path == path) else {
            return Reply::missing();
        };
        let site = &call.site;
        match entry.kind() {
            "dir" => {
                let listing = self
                    .tree(&commit, &format!("{path}/"))
                    .await
                    .unwrap_or_default();
                Reply::ok(Value::Array(
                    listing
                        .iter()
                        .map(|child| site.content_entry(child, reference))
                        .collect(),
                ))
            }
            "submodule" => Reply::ok(site.content_entry(entry, reference)),
            _ if entry.size > CONTENTS_LIMIT_BYTES => {
                Reply::with(403, github::forbidden_too_large(), None)
            }
            _ => match self.git(&["cat-file", "blob", &entry.sha]).await {
                Some(bytes) => Reply::ok(site.content_file(entry, reference, &bytes)),
                None => Reply::missing(),
            },
        }
    }

    async fn tree(&self, commit: &str, path: &str) -> Option<Vec<TreeEntry>> {
        let text = self
            .git_text(&["ls-tree", "-z", "-l", commit, "--", path])
            .await?;
        Some(github::parse_tree(&text))
    }

    async fn cache_list(&self, call: &Call<'_>) -> Reply {
        let Ok(index) = read_index(&self.config.cache_dir).await else {
            return Reply::with(500, json!({"message": "cache index is unreadable"}), None);
        };
        let listed = index.listed(call.site.repository, now());
        let entries = github::cache_list(&listed, &call.params);
        let page = Page::from_query(&call.params, DEFAULT_PER_PAGE);
        Reply::with(
            200,
            json!({"total_count": entries.len(), "actions_caches": page.slice(&entries)}),
            page.link(entries.len(), call.host, call.path, &call.params),
        )
    }

    async fn cache_delete(&self, call: &Call<'_>, id: u64) -> Reply {
        let Ok(mut index) = read_index(&self.config.cache_dir).await else {
            return Reply::with(500, json!({"message": "cache index is unreadable"}), None);
        };
        let Some(record) = index.delete(call.site.repository, id) else {
            return Reply::missing();
        };
        if write_index(&self.config.cache_dir, &index).await.is_err() {
            return Reply::with(500, json!({"message": "cache index is not writable"}), None);
        }
        let _ = fs::remove_file(self.config.cache_dir.join("archives").join(&record.blob)).await;
        let _ = fs::remove_dir_all(self.config.cache_dir.join("blocks").join(&record.blob)).await;
        Reply {
            status: 204,
            body: None,
            link: None,
        }
    }
}

fn paged(call: &Call<'_>, default_per_page: usize, items: &[Value]) -> Reply {
    let page = Page::from_query(&call.params, default_per_page);
    Reply::with(
        200,
        Value::Array(page.slice(items).to_vec()),
        page.link(items.len(), call.host, call.path, &call.params),
    )
}

fn response(reply: Reply) -> Response<Body> {
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
