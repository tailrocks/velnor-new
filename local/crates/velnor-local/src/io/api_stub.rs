// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::PathBuf;

use hyper::header::HOST;
use hyper::{Request, Response};
use serde_json::{Value, json};
use tokio::fs;
use tokio::sync::Mutex;

use super::git::Git;
use super::reply::{Reply, response, text_response};
use super::{
    Body, lock_index, now, read_index, recover_tombstones, remove_if_present, write_index,
};
use crate::rest::{
    self, Api, COMPARE_COMMITS_DEFAULT, COMPARE_FILES_CAP, CONTENTS_LIMIT_BYTES, Commit,
    Comparison, DEFAULT_PER_PAGE, DIRECTORY_LIMIT, MAX_PER_PAGE, PULL_FILES_CAP, Page, Site,
    TreeEntry,
};

mod actions;

pub use actions::{Answer, HttpStandIn, StandIn};

const SYMLINK_TARGET_LIMIT: u64 = 4096;

#[derive(Debug)]
pub struct ApiStubConfig {
    pub repo: PathBuf,
    pub dir: PathBuf,
    pub cache_dir: PathBuf,
    pub default_branch: String,
    pub diff_limit_bytes: usize,
    pub standin: Option<std::sync::Arc<dyn StandIn>>,
}

#[derive(Debug)]
pub struct ApiStub {
    config: ApiStubConfig,
    git: Git,
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
        Self {
            git: Git {
                repo: config.repo.clone(),
                diff_limit_bytes: config.diff_limit_bytes,
                last_diff: Mutex::new(None),
            },
            config,
        }
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
        let repository = rest::repository(&path);
        let call = Call {
            host: &host,
            path: &path,
            params: rest::query(request.uri().query().unwrap_or_default()),
            site: Site {
                host: &host,
                repository: &repository,
            },
        };
        let route = rest::route(&method, &path);
        if let Api::JobLog { id } = route {
            let log = self.job_log(id).await;
            eprintln!("{method} {path} {}", if log.is_some() { 200 } else { 404 });
            return log.map_or_else(|| response(Reply::missing()), text_response);
        }
        let reply = match route {
            Api::PullFiles { number } => self.pull_files(&call, number).await,
            Api::PullList => self.pull_list(&call).await,
            Api::Compare { base, head } => self.compare(&call, &base, &head).await,
            Api::Contents { path } => self.contents(&call, &path).await,
            Api::CacheList => self.cache_list(&call).await,
            Api::CacheDelete { id } => self.cache_delete(&call, id).await,
            Api::Run { id } => self.run(id).await,
            Api::RunJobs { id, attempt } => self.run_jobs(&call, id, attempt).await,
            Api::JobLog { .. } | Api::NotImplemented => {
                Reply::with(501, rest::not_implemented(&method, &path), None)
            }
        };
        eprintln!("{method} {path} {}", reply.status);
        response(reply)
    }

    async fn pulls(&self) -> Option<Vec<Value>> {
        let bytes = fs::read(self.config.dir.join("pulls.json")).await.ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    async fn pull_list(&self, call: &Call<'_>) -> Reply {
        let wanted = call.params.get("state").map_or("open", String::as_str);
        let Some(pulls) = self.pulls().await else {
            return Reply::unreadable_pulls();
        };
        let pulls: Vec<Value> = pulls
            .into_iter()
            .filter(|pull| wanted == "all" || pull["state"].as_str().unwrap_or("open") == wanted)
            .collect();
        paged(call, DEFAULT_PER_PAGE, &pulls)
    }

    async fn pull_files(&self, call: &Call<'_>, number: u64) -> Reply {
        let Some(pulls) = self.pulls().await else {
            return Reply::unreadable_pulls();
        };
        let Some(pull) = pulls.iter().find(|pull| pull["number"] == number) else {
            return Reply::missing();
        };
        let (Some(base), Some(head)) = (pull["base"]["sha"].as_str(), pull["head"]["sha"].as_str())
        else {
            return Reply::missing();
        };
        let (Some(base), Some(head)) = (self.git.resolve(base).await, self.git.resolve(head).await)
        else {
            return Reply::missing();
        };
        let files = match self.git.diff(&base, &head, PULL_FILES_CAP).await {
            Ok(files) => files,
            Err(failure) => return Reply::from(failure),
        };
        let files: Vec<Value> = files
            .iter()
            .take(PULL_FILES_CAP)
            .map(|file| call.site.file(file, &base, &head))
            .collect();
        paged(call, DEFAULT_PER_PAGE, &files)
    }

    async fn compare(&self, call: &Call<'_>, base: &str, head: &str) -> Reply {
        let requested = format!("{base}...{head}");
        let (Some(base), Some(head)) = (self.git.resolve(base).await, self.git.resolve(head).await)
        else {
            return Reply::missing();
        };
        let Some(merge_base) = self
            .git
            .text(&["merge-base", "--end-of-options", &base, &head])
            .await
            .map(|sha| sha.trim().to_owned())
        else {
            return Reply::missing();
        };
        let range = format!("{base}..{head}");
        let behind = format!("{head}..{base}");
        let (Some(ahead), Some(behind)) =
            (self.git.count(&range).await, self.git.count(&behind).await)
        else {
            return Reply::missing();
        };
        let unpaged = !call.params.contains_key("page") && !call.params.contains_key("per_page");
        let page = Page::from_query(&call.params, DEFAULT_PER_PAGE, MAX_PER_PAGE);
        let (skip, take) = if unpaged {
            (0, ahead.min(COMPARE_COMMITS_DEFAULT))
        } else {
            page.newest_first(ahead)
        };
        let files = if page.page == 1 {
            match self.git.diff(&base, &head, COMPARE_FILES_CAP).await {
                Ok(files) => files,
                Err(failure) => return Reply::from(failure),
            }
        } else {
            Vec::new()
        };
        let (Some(commits), Some(base_commit), Some(merge_commit)) = (
            self.git
                .commits(
                    &[&format!("--skip={skip}"), &format!("--max-count={take}")],
                    &range,
                )
                .await,
            self.git.commits(&["-1"], &base).await,
            self.git.commits(&["-1"], &merge_base).await,
        ) else {
            return Reply::missing();
        };
        let commits: Vec<Commit> = commits.into_iter().rev().collect();
        let (Some(base_commit), Some(merge_commit)) = (base_commit.first(), merge_commit.first())
        else {
            return Reply::missing();
        };
        let comparison = Comparison {
            base: base_commit,
            head: &head,
            requested: &requested,
            merge_base: merge_commit,
            ahead_by: ahead,
            behind_by: behind,
            files: &files[..files.len().min(COMPARE_FILES_CAP)],
        };
        let body = call.site.compare(&comparison, page, &commits);
        let link = if unpaged {
            None
        } else {
            page.link(ahead, call.host, call.path, &call.params)
        };
        Reply::with(200, body, link)
    }

    async fn contents(&self, call: &Call<'_>, path: &str) -> Reply {
        let reference = call
            .params
            .get("ref")
            .map_or(self.config.default_branch.as_str(), String::as_str);
        let qualified = if call.params.contains_key("ref") {
            reference.to_owned()
        } else {
            format!("refs/heads/{reference}")
        };
        let Some(commit) = self.git.resolve(&qualified).await else {
            return Reply::missing();
        };
        let path = path.trim_end_matches('/');
        let Some(entries) = self.git.tree(&commit, path).await else {
            return Reply::missing();
        };
        let Some(mut entry) = entries.iter().find(|entry| entry.path == path) else {
            return Reply::missing();
        };
        let followed = self.link_target(&commit, entry).await;
        if let Some(target) = &followed {
            entry = target;
        }
        let site = &call.site;
        match entry.kind() {
            "dir" => {
                let listing = self
                    .git
                    .tree(&commit, &format!("{path}/"))
                    .await
                    .unwrap_or_default();
                Reply::ok(Value::Array(
                    listing
                        .iter()
                        .take(DIRECTORY_LIMIT)
                        .map(|child| site.listing_entry(child, reference))
                        .collect(),
                ))
            }
            "submodule" => Reply::ok(site.content_entry(entry, reference)),
            _ if entry.size > CONTENTS_LIMIT_BYTES => {
                Reply::with(403, rest::forbidden_too_large(), None)
            }
            _ => match self.git.run(&["cat-file", "blob", &entry.sha]).await {
                Some(bytes) => Reply::ok(site.content_file(entry, reference, &bytes)),
                None => Reply::missing(),
            },
        }
    }

    async fn link_target(&self, commit: &str, entry: &TreeEntry) -> Option<TreeEntry> {
        if entry.kind() != "symlink" || entry.size > SYMLINK_TARGET_LIMIT {
            return None;
        }
        let bytes = self.git.run(&["cat-file", "blob", &entry.sha]).await?;
        let target = rest::link_target(&entry.path, &String::from_utf8_lossy(&bytes))?;
        let found = self.git.tree(commit, &target).await?;
        found
            .into_iter()
            .find(|candidate| candidate.path == target && candidate.kind() == "file")
    }

    async fn cache_list(&self, call: &Call<'_>) -> Reply {
        let Ok(index) = read_index(&self.config.cache_dir).await else {
            return Reply::with(500, json!({"message": "cache index is unreadable"}), None);
        };
        let listed = index.listed(call.site.repository, now());
        let entries = rest::cache_list(&listed, &call.params);
        let page = Page::from_query(&call.params, DEFAULT_PER_PAGE, MAX_PER_PAGE);
        Reply::with(
            200,
            json!({"total_count": entries.len(), "actions_caches": page.slice(&entries)}),
            page.link(entries.len(), call.host, call.path, &call.params),
        )
    }

    async fn cache_delete(&self, call: &Call<'_>, id: u64) -> Reply {
        let dir = &self.config.cache_dir;
        let Ok(_lock) = lock_index(dir).await else {
            return Reply::with(500, json!({"message": "cache index is locked"}), None);
        };
        let Ok(mut index) = read_index(dir).await else {
            return Reply::with(500, json!({"message": "cache index is unreadable"}), None);
        };
        recover_tombstones(dir, &index).await;
        let Some(record) = index.delete(call.site.repository, id) else {
            return Reply::missing();
        };
        let archive = dir.join("archives").join(&record.blob);
        let tombstone = dir
            .join("archives")
            .join(format!("{}.deleted", record.blob));
        if remove_if_present(fs::rename(&archive, &tombstone).await).is_err() {
            return Reply::with(
                500,
                json!({"message": "cache archive could not be removed"}),
                None,
            );
        }
        if write_index(dir, &index).await.is_err() {
            let _ = fs::rename(&tombstone, &archive).await;
            return Reply::with(500, json!({"message": "cache index is not writable"}), None);
        }
        let _ = fs::remove_file(&tombstone).await;
        let _ = fs::remove_dir_all(dir.join("blocks").join(&record.blob)).await;
        Reply {
            status: 204,
            body: None,
            link: None,
        }
    }
}

fn paged(call: &Call<'_>, default_per_page: usize, items: &[Value]) -> Reply {
    let page = Page::from_query(&call.params, default_per_page, MAX_PER_PAGE);
    Reply::with(
        200,
        Value::Array(page.slice(items).to_vec()),
        page.link(items.len(), call.host, call.path, &call.params),
    )
}
