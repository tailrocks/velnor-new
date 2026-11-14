// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use percent_encoding::percent_decode_str;
use serde_json::{Value, json};

mod caches;
mod diff;
pub mod runs;
mod site;

pub use caches::{cache_list, rfc3339};
pub use diff::{COMMIT_FORMAT, Commit, FileDiff, TreeEntry, parse_commits, parse_diff, parse_tree};
pub use site::{Comparison, Site, encode_path};

pub const PULL_FILES_CAP: usize = 3000;
pub const COMPARE_FILES_CAP: usize = 300;
pub const COMPARE_COMMITS_DEFAULT: usize = 250;
pub const DEFAULT_PER_PAGE: usize = 30;
pub const MAX_PER_PAGE: usize = 100;
pub const DIRECTORY_LIMIT: usize = 1000;
pub const CONTENTS_LIMIT_BYTES: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Api {
    PullFiles { number: u64 },
    PullList,
    Compare { base: String, head: String },
    Contents { path: String },
    CacheList,
    CacheDelete { id: u64 },
    Run { id: u64 },
    RunJobs { id: u64, attempt: u32 },
    JobLog { id: u64 },
    NotImplemented,
}

#[must_use]
pub fn route(method: &str, path: &str) -> Api {
    let segments: Vec<&str> = path.trim_start_matches('/').splitn(5, '/').collect();
    let route = match (method, segments.as_slice()) {
        ("GET", ["repos", _, _, "pulls"]) => Some(Api::PullList),
        ("GET", ["repos", _, _, "pulls", rest]) => rest
            .strip_suffix("/files")
            .and_then(|number| number.parse().ok())
            .map(|number| Api::PullFiles { number }),
        ("GET", ["repos", _, _, "compare", rest]) => {
            let decoded = percent_decode_str(rest).decode_utf8_lossy();
            decoded
                .split_once("...")
                .filter(|(base, head)| !base.is_empty() && !head.is_empty())
                .map(|(base, head)| Api::Compare {
                    base: base.to_owned(),
                    head: head.to_owned(),
                })
        }
        ("GET", ["repos", _, _, "contents", rest]) => {
            let path = percent_decode_str(rest).decode_utf8_lossy().into_owned();
            (!path.is_empty()).then_some(Api::Contents { path })
        }
        ("GET", ["repos", _, _, "actions", "caches"]) => Some(Api::CacheList),
        ("GET", ["repos", _, _, "actions", rest]) => actions(rest),
        ("DELETE", ["repos", _, _, "actions", rest]) => rest
            .strip_prefix("caches/")
            .and_then(|id| id.parse().ok())
            .map(|id| Api::CacheDelete { id }),
        _ => None,
    };
    route.unwrap_or(Api::NotImplemented)
}

fn actions(rest: &str) -> Option<Api> {
    match rest.split('/').collect::<Vec<_>>().as_slice() {
        ["runs", id] => Some(Api::Run {
            id: id.parse().ok()?,
        }),
        ["runs", id, "attempts", attempt, "jobs"] => Some(Api::RunJobs {
            id: id.parse().ok()?,
            attempt: attempt.parse().ok()?,
        }),
        ["jobs", id, "logs"] => Some(Api::JobLog {
            id: id.parse().ok()?,
        }),
        _ => None,
    }
}

#[must_use]
pub fn repository(path: &str) -> String {
    let mut segments = path.trim_start_matches('/').split('/').skip(1);
    format!(
        "{}/{}",
        segments.next().unwrap_or_default(),
        segments.next().unwrap_or_default()
    )
}

#[must_use]
pub fn not_implemented(method: &str, path: &str) -> Value {
    json!({
        "message": format!("velnor-local api-stub does not implement {method} {path}"),
        "path": path,
        "method": method,
        "status": "501",
    })
}

#[must_use]
pub fn not_found() -> Value {
    json!({"message": "Not Found", "documentation_url": "https://docs.github.com/rest", "status": "404"})
}

#[must_use]
pub fn forbidden_too_large() -> Value {
    json!({
        "message": "This API returns blobs up to 1 MB in size. The requested blob is too large to fetch via the API.",
        "errors": [{"resource": "Blob", "code": "too_large", "field": "data"}],
        "documentation_url": "https://docs.github.com/rest/repos/contents#get-repository-content",
        "status": "403",
    })
}

#[must_use]
pub fn query(raw: &str) -> BTreeMap<String, String> {
    form_urlencoded::parse(raw.as_bytes())
        .into_owned()
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    pub page: usize,
    pub per_page: usize,
}

impl Page {
    #[must_use]
    pub fn from_query(
        params: &BTreeMap<String, String>,
        default_per_page: usize,
        max_per_page: usize,
    ) -> Self {
        let number = |name: &str| {
            params
                .get(name)
                .and_then(|value| value.parse::<usize>().ok())
        };
        Self {
            page: number("page").filter(|page| *page >= 1).unwrap_or(1),
            per_page: number("per_page")
                .filter(|per_page| *per_page >= 1)
                .map_or(default_per_page, |per_page| per_page.min(max_per_page)),
        }
    }

    #[must_use]
    pub fn slice<'a, T>(&self, items: &'a [T]) -> &'a [T] {
        let start = (self.page - 1)
            .saturating_mul(self.per_page)
            .min(items.len());
        &items[start..(start + self.per_page).min(items.len())]
    }

    #[must_use]
    pub fn newest_first(&self, total: usize) -> (usize, usize) {
        let start = (self.page - 1).saturating_mul(self.per_page).min(total);
        let end = start.saturating_add(self.per_page).min(total);
        (total - end, end - start)
    }

    fn last(&self, total: usize) -> usize {
        total.div_ceil(self.per_page).max(1)
    }

    #[must_use]
    pub fn link(
        &self,
        total: usize,
        host: &str,
        path: &str,
        params: &BTreeMap<String, String>,
    ) -> Option<String> {
        let last = self.last(total);
        if last == 1 && self.page == 1 {
            return None;
        }
        let url = |page: usize| {
            let mut params = params.clone();
            params.insert("page".to_owned(), page.to_string());
            params.insert("per_page".to_owned(), self.per_page.to_string());
            let query = form_urlencoded::Serializer::new(String::new())
                .extend_pairs(params)
                .finish();
            format!("http://{host}{path}?{query}")
        };
        let mut links = Vec::new();
        if self.page > 1 {
            links.push((self.page - 1, "prev"));
        }
        if self.page < last {
            links.push((self.page + 1, "next"));
            links.push((last, "last"));
        }
        if self.page > 1 {
            links.push((1, "first"));
        }
        Some(
            links
                .into_iter()
                .map(|(page, rel)| format!("<{}>; rel=\"{rel}\"", url(page)))
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

#[must_use]
pub fn link_target(path: &str, target: &str) -> Option<String> {
    if target.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}
