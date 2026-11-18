// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use ring::digest::{SHA256, digest};
use serde_json::Value;
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite};
use velnor_ci::publish::Request;
use velnor_ci::release::PlainName;

pub(crate) const RELEASE: &str = include_str!("data/release.json");
pub(crate) const REPO: &str = "tailrocks/velnor-new";
pub(crate) const TAG: &str = "velnor-ci-v0.1.80";
pub(crate) const APP: &str = "velnor-release";
pub(crate) const BOT: &str = "velnor-release[bot]";
pub(crate) const FILES: [(&str, &str); 3] = [
    ("velnor-ci-linux-x64", "velnor-ci for linux-x64\n"),
    ("velnor-ci-linux-arm64", "velnor-ci for linux-arm64\n"),
    ("velnor-ci-macos-arm64", "velnor-ci for macos-arm64\n"),
];
pub(crate) const SUMS: &str =
    "2dcc907cafb22be7ea157626eb896a9573a4b7e2ed37aae7f0e6408edd94d528  velnor-ci-linux-x64
fbd8a7732f157b60fd479fb395efc6062051664f97f928ef552d7aed93145c42  velnor-ci-linux-arm64
96a3ba0c5fdf5db7abdee5487ff4cf793a13e76b5c70b7d67802ce8cccce7a70  velnor-ci-macos-arm64
";
pub(crate) const SUMS_SHA256: &str =
    "52d10479b72a301303db6bc1e6ddc5f45ff42d46401960de8025d673b21557b1";
pub(crate) const INTRUDER: &str = "tailrocks-reviewer";
const UPLOADS: &str = "https://uploads.github.com";
const PAGE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Moment {
    FirstUpload,
    Publish,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Intrusion {
    Add(&'static str),
    Swap(&'static str),
    Remove(&'static str),
    KeepDraft,
}

#[derive(Debug)]
pub(crate) struct FakeReleases {
    uploads: String,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    releases: Vec<Value>,
    next: u64,
    requests: usize,
    intrusions: Vec<(Moment, Intrusion)>,
}

fn sha256(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .concat()
}

fn template() -> Value {
    serde_json::from_str(RELEASE).unwrap_or_else(|e| unreachable!("{e}"))
}

fn number(text: &str) -> Result<u64, CiError> {
    text.parse()
        .map_err(|_| CiError::new(format!("404 Not Found: {text}")))
}

fn page(query: &str) -> Result<usize, CiError> {
    let page = query
        .strip_prefix("per_page=100&page=")
        .ok_or_else(|| CiError::new(format!("unexpected query {query}")))?;
    Ok(usize::try_from(number(page)?).unwrap_or(usize::MAX))
}

fn slice(items: &[Value], page: usize) -> String {
    let start = page.saturating_sub(1).saturating_mul(PAGE).min(items.len());
    let end = start.saturating_add(PAGE).min(items.len());
    Value::Array(items[start..end].to_vec()).to_string()
}

impl State {
    fn release(&mut self, id: u64) -> Result<&mut Value, CiError> {
        self.releases
            .iter_mut()
            .find(|release| release["id"] == id)
            .ok_or_else(|| CiError::new(format!("404 Not Found: release {id}")))
    }

    fn add_asset(&mut self, id: u64, name: &str, bytes: &[u8], uploader: &str) -> Value {
        self.next += 1;
        let mut asset = template()["assets"][0].clone();
        asset["id"] = self.next.into();
        asset["name"] = name.into();
        asset["size"] = bytes.len().into();
        asset["state"] = "uploaded".into();
        asset["digest"] = format!("sha256:{}", sha256(bytes)).into();
        asset["uploader"]["login"] = uploader.into();
        if let Ok(release) = self.release(id)
            && let Some(assets) = release["assets"].as_array_mut()
        {
            assets.push(asset.clone());
        }
        asset
    }

    fn assets_of(&mut self, id: u64) -> Option<&mut Vec<Value>> {
        self.release(id).ok()?["assets"].as_array_mut()
    }

    fn intrude(&mut self, id: u64, moment: Moment) -> bool {
        let due: Vec<Intrusion> = self
            .intrusions
            .iter()
            .filter(|(when, _)| *when == moment)
            .map(|(_, intrusion)| *intrusion)
            .collect();
        self.intrusions.retain(|(when, _)| *when != moment);
        let mut keep_draft = false;
        for intrusion in due {
            match intrusion {
                Intrusion::Add(name) => {
                    self.add_asset(id, name, b"planted", INTRUDER);
                }
                Intrusion::Swap(name) => {
                    for asset in self.assets_of(id).into_iter().flatten() {
                        if asset["name"] == name {
                            asset["digest"] = format!("sha256:{}", sha256(b"planted")).into();
                        }
                    }
                }
                Intrusion::Remove(name) => {
                    if let Some(assets) = self.assets_of(id) {
                        assets.retain(|asset| asset["name"] != name);
                    }
                }
                Intrusion::KeepDraft => keep_draft = true,
            }
        }
        keep_draft
    }
}

impl FakeReleases {
    pub(crate) fn new() -> Self {
        Self::uploading_to(UPLOADS)
    }

    pub(crate) fn uploading_to(uploads: &str) -> Self {
        Self {
            uploads: uploads.to_owned(),
            state: Mutex::new(State {
                next: 900_000_000,
                ..State::default()
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn release(&self, tag: &str, draft: bool, login: &str, kind: &str) -> u64 {
        let mut state = self.state();
        state.next += 1;
        let id = state.next;
        let mut release = template();
        release["id"] = id.into();
        release["tag_name"] = tag.into();
        release["name"] = tag.into();
        release["draft"] = draft.into();
        release["author"]["login"] = login.into();
        release["author"]["type"] = kind.into();
        release["assets"] = Value::Array(Vec::new());
        release["upload_url"] = format!(
            "{}/repos/{REPO}/releases/{id}/assets{{?name,label}}",
            self.uploads
        )
        .into();
        state.releases.push(release);
        id
    }

    pub(crate) fn app_draft(&self) -> u64 {
        self.release(TAG, true, BOT, "Bot")
    }

    pub(crate) fn others(&self, count: usize) {
        for index in 0..count {
            self.release(
                &format!("rust-ci-v0.0.{index}"),
                index % 7 == 0,
                "github-actions[bot]",
                "Bot",
            );
        }
    }

    pub(crate) fn asset(&self, id: u64, name: &str, bytes: &[u8]) -> u64 {
        self.state().add_asset(id, name, bytes, INTRUDER)["id"]
            .as_u64()
            .unwrap_or_default()
    }

    pub(crate) fn upload_into(&self, id: u64, other: u64) {
        let uploads = &self.uploads;
        if let Ok(release) = self.state().release(id) {
            release["upload_url"] =
                format!("{uploads}/repos/{REPO}/releases/{other}/assets{{?name,label}}").into();
        }
    }

    pub(crate) fn intrude(&self, moment: Moment, intrusion: Intrusion) {
        self.state().intrusions.push((moment, intrusion));
    }

    pub(crate) fn is_draft(&self, id: u64) -> bool {
        self.state()
            .release(id)
            .is_ok_and(|release| release["draft"] == true)
    }

    pub(crate) fn assets(&self, id: u64) -> BTreeMap<String, String> {
        let mut state = self.state();
        let Ok(release) = state.release(id) else {
            return BTreeMap::new();
        };
        release["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|asset| {
                (
                    asset["name"].as_str().unwrap_or_default().to_owned(),
                    asset["digest"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }

    pub(crate) fn requests(&self) -> usize {
        self.state().requests
    }
}

impl GitHub for FakeReleases {
    fn get(&self, path: &str) -> Result<String, CiError> {
        let mut state = self.state();
        state.requests += 1;
        let rest = path
            .strip_prefix(&format!("/repos/{REPO}/releases"))
            .ok_or_else(|| CiError::new(format!("404 Not Found: {path}")))?;
        if let Some(query) = rest.strip_prefix('?') {
            return Ok(slice(&state.releases, page(query)?));
        }
        let (id, query) = rest
            .strip_prefix('/')
            .and_then(|rest| rest.split_once("/assets?"))
            .ok_or_else(|| CiError::new(format!("404 Not Found: {path}")))?;
        let release = state.release(number(id)?)?;
        let assets = release["assets"].as_array().cloned().unwrap_or_default();
        Ok(slice(&assets, page(query)?))
    }
}

impl GitHubWrite for FakeReleases {
    fn wait(&self, _: std::time::Duration) {}

    fn upload(&self, url: &str, bytes: &[u8]) -> Result<String, CiError> {
        let mut state = self.state();
        state.requests += 1;
        let (id, name) = url
            .strip_prefix(&format!("{}/repos/{REPO}/releases/", self.uploads))
            .and_then(|rest| rest.split_once("/assets?name="))
            .ok_or_else(|| CiError::new(format!("404 Not Found: {url}")))?;
        let id = number(id)?;
        let release = state.release(id)?;
        if release["draft"] != true {
            return Err(CiError::new(format!(
                "422 Unprocessable Entity: release {id} is immutable"
            )));
        }
        let taken = release["assets"]
            .as_array()
            .is_some_and(|assets| assets.iter().any(|asset| asset["name"] == name));
        if taken {
            return Err(CiError::new(format!(
                "422 Unprocessable Entity: already_exists {name}"
            )));
        }
        let asset = state.add_asset(id, name, bytes, BOT);
        let _keep_draft = state.intrude(id, Moment::FirstUpload);
        Ok(asset.to_string())
    }

    fn patch(&self, path: &str, json: &str) -> Result<String, CiError> {
        let mut state = self.state();
        state.requests += 1;
        let id = path
            .strip_prefix(&format!("/repos/{REPO}/releases/"))
            .ok_or_else(|| CiError::new(format!("404 Not Found: {path}")))
            .and_then(number)?;
        let body: Value = serde_json::from_str(json)
            .map_err(|e| CiError::new(format!("400 Bad Request: {e}")))?;
        let keep_draft = state.intrude(id, Moment::Publish);
        let release = state.release(id)?;
        if let Some(draft) = body.get("draft").filter(|_| !keep_draft) {
            release["draft"] = draft.clone();
        }
        Ok(release.to_string())
    }

    fn delete(&self, path: &str) -> Result<(), CiError> {
        let mut state = self.state();
        state.requests += 1;
        let asset = path
            .strip_prefix(&format!("/repos/{REPO}/releases/assets/"))
            .ok_or_else(|| CiError::new(format!("404 Not Found: {path}")))
            .and_then(number)?;
        for release in &mut state.releases {
            if let Some(assets) = release["assets"].as_array_mut()
                && let Some(index) = assets.iter().position(|item| item["id"] == asset)
            {
                assets.remove(index);
                return Ok(());
            }
        }
        Err(CiError::new(format!("404 Not Found: {path}")))
    }
}

pub(crate) fn name(text: &str) -> PlainName {
    PlainName::new("name", text).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn request(files: &[(&str, &str)]) -> Request {
    Request {
        tag: name(TAG),
        app: name(APP),
        names: FILES.iter().map(|(file, _)| name(file)).collect(),
        assetless: false,
        files: files
            .iter()
            .map(|(file, text)| ((*file).to_owned(), text.as_bytes().to_vec()))
            .collect(),
    }
}

pub(crate) fn all_files() -> Vec<(&'static str, &'static str)> {
    FILES
        .iter()
        .copied()
        .chain([("SHA256SUMS", SUMS)])
        .collect()
}

pub(crate) fn published_digests() -> BTreeMap<String, String> {
    SUMS.lines()
        .filter_map(|line| line.split_once("  "))
        .map(|(hex, file)| (file.to_owned(), hex.to_owned()))
        .chain([("SHA256SUMS".to_owned(), SUMS_SHA256.to_owned())])
        .map(|(file, hex)| (file, format!("sha256:{hex}")))
        .collect()
}
