// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Value, json};

use super::{COMPARE_FILES_CAP, Commit, FileDiff, Page, TreeEntry};

const PATH_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

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
    pub fn listing_entry(&self, entry: &TreeEntry, reference: &str) -> Value {
        let mut value = self.content_entry(entry, reference);
        if entry.kind() == "submodule" {
            value["type"] = json!("file");
        }
        value
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

#[derive(Debug)]
pub struct Comparison<'a> {
    pub base: &'a Commit,
    pub head: &'a str,
    pub merge_base: &'a Commit,
    pub commits: &'a [Commit],
    pub behind_by: usize,
    pub files: &'a [FileDiff],
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
