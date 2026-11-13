// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use serde_json::json;

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
    let renamed_from = find("rename from ").map(path_text);
    let filename = find("rename to ").map_or_else(|| same_name(names), path_text);
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
        previous_filename: renamed_from,
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
    if names.starts_with('"') {
        let first = unquote(names).map_or_else(String::new, |(path, _)| path);
        return first.strip_prefix("a/").unwrap_or(&first).to_owned();
    }
    let names = names.strip_prefix("a/").unwrap_or(names);
    names[..names.len().saturating_sub(3) / 2].to_owned()
}

fn path_text(text: &str) -> String {
    if text.starts_with('"') {
        unquote(text).map_or_else(|| text.to_owned(), |(path, _)| path)
    } else {
        text.to_owned()
    }
}

fn unquote(text: &str) -> Option<(String, &str)> {
    let body = text.strip_prefix('"')?;
    let mut bytes = Vec::new();
    let mut chars = body.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => {
                return Some((
                    String::from_utf8_lossy(&bytes).into_owned(),
                    &body[at + 1..],
                ));
            }
            '\\' => {
                let (_, escape) = chars.next()?;
                let value = match escape {
                    'a' => 7,
                    'b' => 8,
                    't' => 9,
                    'n' => 10,
                    'v' => 11,
                    'f' => 12,
                    'r' => 13,
                    '0'..='3' => {
                        let digits: String = std::iter::once(escape)
                            .chain(chars.by_ref().take(2).map(|(_, digit)| digit))
                            .collect();
                        u8::from_str_radix(&digits, 8).ok()?
                    }
                    other => u8::try_from(other).ok()?,
                };
                bytes.push(value);
            }
            other => bytes.extend_from_slice(other.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    None
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

pub const COMMIT_FORMAT: &str = "%H%x00%T%x00%P%x00%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI%x00%B";

#[must_use]
pub fn parse_commits(text: &str) -> Vec<Commit> {
    let fields: Vec<&str> = text.split('\0').collect();
    fields
        .chunks(10)
        .filter_map(|record| {
            let [sha, tree, parents, an, ae, ad, cn, ce, cd, message] = record else {
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
                message: message.strip_suffix('\n').unwrap_or(message).to_owned(),
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
