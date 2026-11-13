// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde_json::{Value, json};

use crate::store::Record;

pub const PULL_FILES_CAP: usize = 3000;
pub const COMPARE_FILES_CAP: usize = 300;
pub const COMPARE_COMMITS_DEFAULT: usize = 250;
pub const DEFAULT_PER_PAGE: usize = 30;
pub const MAX_PER_PAGE: usize = 100;
pub const CONTENTS_LIMIT_BYTES: u64 = 1 << 20;

const PATH_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Api {
    PullFiles { number: u64 },
    PullList,
    Compare { base: String, head: String },
    Contents { path: String },
    CacheList,
    CacheDelete { id: u64 },
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
        ("DELETE", ["repos", _, _, "actions", rest]) => rest
            .strip_prefix("caches/")
            .and_then(|id| id.parse().ok())
            .map(|id| Api::CacheDelete { id }),
        _ => None,
    };
    route.unwrap_or(Api::NotImplemented)
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
    pub fn from_query(params: &BTreeMap<String, String>, default_per_page: usize) -> Self {
        let number = |name: &str| {
            params
                .get(name)
                .and_then(|value| value.parse::<usize>().ok())
        };
        Self {
            page: number("page").filter(|page| *page >= 1).unwrap_or(1),
            per_page: number("per_page")
                .filter(|per_page| *per_page >= 1)
                .map_or(default_per_page, |per_page| per_page.min(MAX_PER_PAGE)),
        }
    }

    #[must_use]
    pub fn slice<'a, T>(&self, items: &'a [T]) -> &'a [T] {
        let start = (self.page - 1)
            .saturating_mul(self.per_page)
            .min(items.len());
        &items[start..(start + self.per_page).min(items.len())]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    pub filename: String,
    pub previous_filename: Option<String>,
    pub status: &'static str,
    pub sha: Option<String>,
    pub additions: usize,
    pub deletions: usize,
    pub patch: Option<String>,
}

#[must_use]
pub fn parse_diff(text: &str) -> Vec<FileDiff> {
    text.split("\ndiff --git ")
        .map(|section| section.strip_prefix("diff --git ").unwrap_or(section))
        .filter(|section| !section.trim().is_empty())
        .map(file_diff)
        .collect()
}

fn file_diff(section: &str) -> FileDiff {
    let mut lines = section.lines();
    let names = lines.next().unwrap_or_default();
    let mut header = Vec::new();
    let mut hunks = Vec::new();
    for line in lines {
        if hunks.is_empty() && !line.starts_with("@@") {
            header.push(line);
        } else {
            hunks.push(line);
        }
    }
    let find = |prefix: &str| header.iter().find_map(|line| line.strip_prefix(prefix));
    let renamed_from = find("rename from ");
    let filename = find("rename to ").map_or_else(|| same_name(names), str::to_owned);
    let (old_sha, new_sha) = find("index ")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|shas| shas.split_once(".."))
        .map_or((None, None), |(old, new)| {
            (Some(old.to_owned()), Some(new.to_owned()))
        });
    let status = if find("new file mode ").is_some() {
        "added"
    } else if find("deleted file mode ").is_some() {
        "removed"
    } else if renamed_from.is_some() {
        "renamed"
    } else if find("old mode ").is_some() && hunks.is_empty() {
        "changed"
    } else {
        "modified"
    };
    let count = |sign: char| hunks.iter().filter(|line| line.starts_with(sign)).count();
    FileDiff {
        filename,
        previous_filename: renamed_from.map(str::to_owned),
        status,
        sha: if status == "removed" {
            old_sha
        } else {
            new_sha
        },
        additions: count('+'),
        deletions: count('-'),
        patch: (!hunks.is_empty()).then(|| hunks.join("\n")),
    }
}

fn same_name(names: &str) -> String {
    let names = names.strip_prefix("a/").unwrap_or(names);
    names[..names.len().saturating_sub(3) / 2].to_owned()
}

#[must_use]
pub fn encode_path(path: &str) -> String {
    utf8_percent_encode(path, PATH_SET).to_string()
}

#[derive(Clone, Copy, Debug)]
pub struct Site<'a> {
    pub host: &'a str,
    pub repository: &'a str,
}

impl Site<'_> {
    fn blob_url(&self, kind: &str, sha: &str, path: &str) -> String {
        format!(
            "http://{}/{}/{kind}/{sha}/{}",
            self.host,
            self.repository,
            encode_path(path)
        )
    }

    fn contents_url(&self, sha: &str, path: &str) -> String {
        format!(
            "http://{}/repos/{}/contents/{}?ref={sha}",
            self.host,
            self.repository,
            encode_path(path)
        )
    }

    #[must_use]
    pub fn file(&self, file: &FileDiff, head: &str) -> Value {
        let mut value = json!({
            "sha": file.sha,
            "filename": file.filename,
            "status": file.status,
            "additions": file.additions,
            "deletions": file.deletions,
            "changes": file.additions + file.deletions,
            "blob_url": self.blob_url("blob", head, &file.filename),
            "raw_url": self.blob_url("raw", head, &file.filename),
            "contents_url": self.contents_url(head, &file.filename),
        });
        if let Some(patch) = &file.patch {
            value["patch"] = json!(patch);
        }
        if let Some(previous) = &file.previous_filename {
            value["previous_filename"] = json!(previous);
        }
        value
    }

    #[must_use]
    pub fn commit(&self, commit: &Commit) -> Value {
        let api = format!("http://{}/repos/{}", self.host, self.repository);
        json!({
            "sha": commit.sha,
            "url": format!("{api}/commits/{}", commit.sha),
            "html_url": format!("http://{}/{}/commit/{}", self.host, self.repository, commit.sha),
            "commit": {
                "url": format!("{api}/git/commits/{}", commit.sha),
                "author": commit.author,
                "committer": commit.committer,
                "message": commit.message,
                "tree": {"sha": commit.tree, "url": format!("{api}/git/trees/{}", commit.tree)},
                "comment_count": 0,
            },
            "parents": commit.parents.iter().map(|sha| json!({
                "sha": sha,
                "url": format!("{api}/commits/{sha}"),
                "html_url": format!("http://{}/{}/commit/{sha}", self.host, self.repository),
            })).collect::<Vec<_>>(),
        })
    }

    #[must_use]
    pub fn compare(&self, compare: &Comparison<'_>, page: Page, commits: &[Commit]) -> Value {
        let spec = format!("{}...{}", compare.base.sha, compare.head);
        let web = format!("http://{}/{}/compare", self.host, self.repository);
        let ahead = compare.commits.len();
        let status = match (ahead, compare.behind_by) {
            (0, 0) => "identical",
            (_, 0) => "ahead",
            (0, _) => "behind",
            _ => "diverged",
        };
        let files: Vec<Value> = if page.page == 1 {
            compare
                .files
                .iter()
                .take(COMPARE_FILES_CAP)
                .map(|file| self.file(file, compare.head))
                .collect()
        } else {
            Vec::new()
        };
        json!({
            "url": format!("http://{}/repos/{}/compare/{spec}", self.host, self.repository),
            "html_url": format!("{web}/{spec}"),
            "permalink_url": format!("{web}/{spec}"),
            "diff_url": format!("{web}/{spec}.diff"),
            "patch_url": format!("{web}/{spec}.patch"),
            "base_commit": self.commit(compare.base),
            "merge_base_commit": self.commit(compare.merge_base),
            "status": status,
            "ahead_by": ahead,
            "behind_by": compare.behind_by,
            "total_commits": ahead,
            "commits": commits.iter().map(|commit| self.commit(commit)).collect::<Vec<_>>(),
            "files": files,
        })
    }

    #[must_use]
    pub fn content_entry(&self, entry: &TreeEntry, reference: &str) -> Value {
        let url = self.contents_url(reference, &entry.path);
        let name = entry.path.rsplit('/').next().unwrap_or_default();
        let kind = entry.kind();
        let html = self.blob_url(
            if kind == "dir" { "tree" } else { "blob" },
            reference,
            &entry.path,
        );
        let git = format!(
            "http://{}/repos/{}/git/{}/{}",
            self.host,
            self.repository,
            if kind == "dir" { "trees" } else { "blobs" },
            entry.sha
        );
        json!({
            "type": kind,
            "size": if kind == "file" || kind == "symlink" { entry.size } else { 0 },
            "name": name,
            "path": entry.path,
            "sha": entry.sha,
            "url": url,
            "git_url": git,
            "html_url": html,
            "download_url": (kind == "file").then(|| self.blob_url("raw", reference, &entry.path)),
            "_links": {"self": url, "git": git, "html": html},
        })
    }

    #[must_use]
    pub fn content_file(&self, entry: &TreeEntry, reference: &str, bytes: &[u8]) -> Value {
        let mut value = self.content_entry(entry, reference);
        if entry.kind() == "symlink" {
            value["target"] = json!(String::from_utf8_lossy(bytes));
        } else {
            value["encoding"] = json!("base64");
            value["content"] = json!(base64_lines(bytes));
        }
        value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub tree: String,
    pub parents: Vec<String>,
    pub author: Value,
    pub committer: Value,
    pub message: String,
}

#[derive(Debug)]
pub struct Comparison<'a> {
    pub base: &'a Commit,
    pub head: &'a str,
    pub merge_base: &'a Commit,
    pub commits: &'a [Commit],
    pub behind_by: usize,
    pub files: &'a [FileDiff],
}

pub const COMMIT_FORMAT: &str =
    "%H%x00%T%x00%P%x00%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI%x00%B%x01";

#[must_use]
pub fn parse_commits(text: &str) -> Vec<Commit> {
    text.split('\u{1}')
        .map(|record| record.trim_start_matches('\n'))
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            let fields: Vec<&str> = record.splitn(10, '\0').collect();
            let [sha, tree, parents, an, ae, ad, cn, ce, cd, message] = fields.as_slice() else {
                return None;
            };
            let person = |name: &str, email: &str, date: &str| {
                json!({"name": name, "email": email, "date": date})
            };
            Some(Commit {
                sha: (*sha).to_owned(),
                tree: (*tree).to_owned(),
                parents: parents.split_whitespace().map(str::to_owned).collect(),
                author: person(an, ae, ad),
                committer: person(cn, ce, cd),
                message: (*message).to_owned(),
            })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: String,
    pub sha: String,
    pub size: u64,
    pub path: String,
}

impl TreeEntry {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self.mode.as_str() {
            "040000" => "dir",
            "160000" => "submodule",
            "120000" => "symlink",
            _ => "file",
        }
    }
}

#[must_use]
pub fn parse_tree(text: &str) -> Vec<TreeEntry> {
    text.split('\0')
        .filter_map(|entry| {
            let (meta, path) = entry.split_once('\t')?;
            let mut fields = meta.split_whitespace();
            let mode = fields.next()?.to_owned();
            let _kind = fields.next()?;
            let sha = fields.next()?.to_owned();
            let size = fields.next()?.parse().unwrap_or(0);
            Some(TreeEntry {
                mode,
                sha,
                size,
                path: path.to_owned(),
            })
        })
        .collect()
}

fn base64_lines(bytes: &[u8]) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    encoded
        .as_bytes()
        .chunks(60)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned() + "\n")
        .collect()
}

#[must_use]
pub fn rfc3339(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let rest = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Created,
    LastAccessed,
    Size,
}

pub fn cache_list(records: &[Record], params: &BTreeMap<String, String>) -> Vec<Value> {
    let mut matching: Vec<&Record> = records
        .iter()
        .filter(|record| {
            params
                .get("ref")
                .is_none_or(|git_ref| *git_ref == record.scope)
        })
        .filter(|record| {
            params
                .get("key")
                .is_none_or(|key| record.key.starts_with(key))
        })
        .collect();
    let sort = match params.get("sort").map(String::as_str) {
        Some("created_at") => Sort::Created,
        Some("size_in_bytes") => Sort::Size,
        _ => Sort::LastAccessed,
    };
    matching.sort_by_key(|record| {
        (
            match sort {
                Sort::Created => record.created,
                Sort::LastAccessed => record.last_access,
                Sort::Size => record.size_bytes,
            },
            record.id,
        )
    });
    if params.get("direction").map(String::as_str) != Some("asc") {
        matching.reverse();
    }
    matching
        .into_iter()
        .map(|record| {
            json!({
                "id": record.id,
                "ref": record.scope,
                "key": record.key,
                "version": record.version,
                "last_accessed_at": rfc3339(record.last_access),
                "created_at": rfc3339(record.created),
                "size_in_bytes": record.size_bytes,
            })
        })
        .collect()
}
