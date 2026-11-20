// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::error::CiError;
use crate::github::{GitHub, GitHubWrite, Repo, parse};
use crate::release::PlainName;
use velnor_rust::sha256_hex;

pub const SUMS: &str = "SHA256SUMS";
const PAGE: usize = 100;
const UPLOAD_TEMPLATE: &str = "{?name,label}";
const UPLOADED: &str = "uploaded";

#[derive(Debug)]
pub struct Request {
    pub tag: PlainName,
    pub app: PlainName,
    pub names: Vec<PlainName>,
    /// Files published when present in `files` and skipped, with a notice, when not.
    pub optional: Vec<PlainName>,
    pub files: BTreeMap<String, Vec<u8>>,
    pub assetless: bool,
}

struct File<'a> {
    name: &'a str,
    sha256: String,
    bytes: &'a [u8],
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiRelease {
    pub(crate) id: u64,
    pub(crate) tag_name: String,
    pub(crate) draft: bool,
    author: Option<ApiAuthor>,
    upload_url: String,
}

#[derive(Debug, Deserialize)]
struct ApiAuthor {
    login: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct ApiAsset {
    id: u64,
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
}

pub fn publish(
    github: &dyn GitHubWrite,
    repo: &Repo,
    request: &Request,
) -> Result<String, CiError> {
    let (files, skipped) = verify(request)?;
    let release = match draft(github, repo, request)? {
        Found::Draft(release) => release,
        Found::PublishedByApp(release) => return confirm_published(github, repo, &release),
    };
    let named = format!("release {} ({})", release.tag_name, release.id);
    let upload = upload_url(repo, &release)?;
    let present = assets(github, repo, release.id)?;
    let foreign: Vec<&str> = present
        .iter()
        .map(|asset| asset.name.as_str())
        .filter(|name| !files.iter().any(|file| file.name == *name))
        .collect();
    if !foreign.is_empty() {
        return Err(CiError::new(format!(
            "draft {named} holds {}, which is not one of its files",
            foreign.join(", ")
        )));
    }
    let mut report = skipped;
    for file in &files {
        match present.iter().find(|asset| asset.name == file.name) {
            Some(asset) if holds(asset, file) => {
                report.push(format!("kept {}", file.name));
                continue;
            }
            Some(asset) => {
                github.delete(&format!("/repos/{repo}/releases/assets/{}", asset.id))?;
                report.push(format!(
                    "removed {} ({}), which did not hold the bytes {SUMS} lists",
                    file.name, asset.id
                ));
            }
            None => {}
        }
        github.upload(&format!("{upload}?name={}", file.name), file.bytes)?;
        report.push(format!("uploaded {}", file.name));
    }
    exactly(github, repo, release.id, &files)
        .map_err(|reason| CiError::new(format!("draft {named} stays a draft: {reason}")))?;
    let published: ApiRelease = parse(
        &github.patch(
            &format!("/repos/{repo}/releases/{}", release.id),
            r#"{"draft":false}"#,
        )?,
        "release",
    )?;
    if published.id != release.id || published.draft || published.tag_name != release.tag_name {
        return Err(CiError::new(format!(
            "GitHub did not publish draft {named}"
        )));
    }
    exactly(github, repo, release.id, &files).map_err(|reason| {
        CiError::new(format!(
            "{named} is published, and {reason}; check it by hand"
        ))
    })?;
    report.push(format!("published {named} with {} files", files.len()));
    Ok(report.join("\n") + "\n")
}

fn verify(request: &Request) -> Result<(Vec<File<'_>>, Vec<String>), CiError> {
    if request.assetless {
        return if request.names.is_empty() && request.optional.is_empty() {
            Ok((Vec::new(), Vec::new()))
        } else {
            Err(CiError::new("an assetless release names no files"))
        };
    }
    let mut names = BTreeSet::new();
    for name in request.names.iter().chain(&request.optional) {
        if name.as_str() == SUMS {
            return Err(CiError::new(format!(
                "{SUMS} is published with every release; do not name it as a file"
            )));
        }
        if !names.insert(name.as_str()) {
            return Err(CiError::new(format!("file {name} is named twice")));
        }
    }
    let mut skipped = Vec::new();
    for name in &request.optional {
        if !request.files.contains_key(name.as_str()) {
            names.remove(name.as_str());
            skipped.push(format!("skipped optional {name}: not found"));
        }
    }
    if request.names.is_empty() {
        return Err(CiError::new("name at least one file to publish"));
    }
    let sums_bytes = request
        .files
        .get(SUMS)
        .ok_or_else(|| CiError::new(format!("{SUMS}: missing")))?;
    let sums = read_sums(sums_bytes)?;
    let mut problems = Vec::new();
    let mut files = Vec::new();
    for name in &names {
        let Some(bytes) = request.files.get(*name) else {
            problems.push(format!("{name}: missing"));
            continue;
        };
        let actual = sha256_hex(bytes);
        match sums.get(*name) {
            None => problems.push(format!("{name}: not listed in {SUMS}")),
            Some(listed) if *listed != actual => problems.push(format!(
                "{name}: FAILED, its SHA-256 is {actual} and {SUMS} lists {listed}"
            )),
            Some(_) => files.push(File {
                name,
                sha256: actual,
                bytes,
            }),
        }
    }
    problems.extend(
        sums.keys()
            .filter(|listed| !names.contains(listed.as_str()))
            .map(|listed| format!("{listed}: listed in {SUMS} and not a file of this release")),
    );
    if !problems.is_empty() {
        return Err(CiError::new(problems.join("\n")));
    }
    files.push(File {
        name: SUMS,
        sha256: sha256_hex(sums_bytes),
        bytes: sums_bytes,
    });
    Ok((files, skipped))
}

fn read_sums(bytes: &[u8]) -> Result<BTreeMap<String, String>, CiError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| CiError::new(format!("{SUMS} is not UTF-8")))?;
    let body = text
        .strip_suffix('\n')
        .ok_or_else(|| CiError::new(format!("{SUMS} is empty or does not end with a newline")))?;
    let mut sums = BTreeMap::new();
    for (index, line) in body.split('\n').enumerate() {
        let (sha256, name) = sum_line(line).ok_or_else(|| {
            CiError::new(format!(
                "line {} of {SUMS} is not a lowercase hex SHA-256, two spaces or a space and '*', and a file name",
                index + 1
            ))
        })?;
        if sums.insert(name.to_owned(), sha256.to_owned()).is_some() {
            return Err(CiError::new(format!("{SUMS} lists {name} twice")));
        }
    }
    Ok(sums)
}

fn sum_line(line: &str) -> Option<(&str, &str)> {
    let (sha256, rest) = line.split_at_checked(64)?;
    let name = rest
        .strip_prefix("  ")
        .or_else(|| rest.strip_prefix(" *"))?;
    let hex = sha256
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    (hex && PlainName::new("file", name).is_ok()).then_some((sha256, name))
}

enum Found {
    Draft(ApiRelease),
    PublishedByApp(ApiRelease),
}

fn confirm_published(
    github: &dyn GitHub,
    repo: &Repo,
    release: &ApiRelease,
) -> Result<String, CiError> {
    let named = format!("release {} ({})", release.tag_name, release.id);
    let present = assets(github, repo, release.id)?;
    if present.is_empty() {
        return Ok(format!("{named} was already published without files\n"));
    }
    let listed: Vec<&str> = present.iter().map(|asset| asset.name.as_str()).collect();
    Err(CiError::new(format!(
        "{named} is already published and holds {}, which an assetless release does not",
        listed.join(", ")
    )))
}

fn draft(github: &dyn GitHub, repo: &Repo, request: &Request) -> Result<Found, CiError> {
    let (tag, app) = (&request.tag, &request.app);
    let mut tagged: Vec<ApiRelease> = pages(github, "releases", |page| {
        format!("/repos/{repo}/releases?per_page={PAGE}&page={page}")
    })?;
    tagged.retain(|release| release.tag_name == tag.as_str());
    let bot = format!("{app}[bot]");
    if let [only] = tagged.as_slice()
        && request.assetless
        && !only.draft
        && made_by(only, &bot)
    {
        return Ok(Found::PublishedByApp(tagged.remove(0)));
    }
    if let Some(published) = tagged.iter().find(|release| !release.draft) {
        return Err(CiError::new(format!(
            "release {tag} is already published ({}), and a tag is published once",
            published.id
        )));
    }
    if tagged.len() > 1 {
        let listed: Vec<String> = tagged
            .iter()
            .map(|release| format!("{} by {}", release.id, author(release)))
            .collect();
        return Err(CiError::new(format!(
            "{} draft releases have the tag {tag} ({}); only one, created by {bot}, may",
            tagged.len(),
            listed.join(", ")
        )));
    }
    let release = tagged
        .pop()
        .ok_or_else(|| CiError::new(format!("no draft release has the tag {tag}")))?;
    if made_by(&release, &bot) {
        Ok(Found::Draft(release))
    } else {
        Err(CiError::new(format!(
            "draft release {tag} ({}) was created by {}, not by {bot}",
            release.id,
            author(&release)
        )))
    }
}

pub(crate) fn made_by(release: &ApiRelease, bot: &str) -> bool {
    release
        .author
        .as_ref()
        .is_some_and(|author| author.login == bot && author.kind == "Bot")
}

pub(crate) fn author(release: &ApiRelease) -> String {
    release.author.as_ref().map_or_else(
        || "nobody".to_owned(),
        |author| format!("{} ({})", author.login, author.kind),
    )
}

fn upload_url(repo: &Repo, release: &ApiRelease) -> Result<String, CiError> {
    let own = format!("/repos/{repo}/releases/{}/assets", release.id).to_ascii_lowercase();
    release
        .upload_url
        .strip_suffix(UPLOAD_TEMPLATE)
        .filter(|base| base.to_ascii_lowercase().ends_with(&own))
        .map(str::to_owned)
        .ok_or_else(|| {
            CiError::new(format!(
                "draft release {} ({}) has the upload URL {}, which does not name it",
                release.tag_name, release.id, release.upload_url
            ))
        })
}

fn assets(github: &dyn GitHub, repo: &Repo, id: u64) -> Result<Vec<ApiAsset>, CiError> {
    pages(github, "release assets", |page| {
        format!("/repos/{repo}/releases/{id}/assets?per_page={PAGE}&page={page}")
    })
}

pub(crate) fn pages<T: DeserializeOwned>(
    github: &dyn GitHub,
    what: &str,
    path: impl Fn(u32) -> String,
) -> Result<Vec<T>, CiError> {
    let mut all = Vec::new();
    for page in 1.. {
        let items: Vec<T> = parse(&github.get(&path(page))?, what)?;
        let last = items.len() < PAGE;
        all.extend(items);
        if last {
            break;
        }
    }
    Ok(all)
}

fn holds(asset: &ApiAsset, file: &File<'_>) -> bool {
    asset.state == UPLOADED
        && asset.size == file.bytes.len() as u64
        && asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            == Some(file.sha256.as_str())
}

fn exactly(github: &dyn GitHub, repo: &Repo, id: u64, files: &[File<'_>]) -> Result<(), CiError> {
    let present = assets(github, repo, id)?;
    let mut problems: Vec<String> = present
        .iter()
        .filter(|asset| !files.iter().any(|file| file.name == asset.name))
        .map(|asset| format!("it holds {}, which is not one of its files", asset.name))
        .collect();
    for file in files {
        let same: Vec<&ApiAsset> = present
            .iter()
            .filter(|asset| asset.name == file.name)
            .collect();
        match same.as_slice() {
            [asset] if holds(asset, file) => {}
            [] => problems.push(format!("it does not hold {}", file.name)),
            _ => problems.push(format!(
                "its {} is not the {} bytes with SHA-256 {} that {SUMS} lists",
                file.name,
                file.bytes.len(),
                file.sha256
            )),
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(CiError::new(problems.join("; ")))
    }
}
