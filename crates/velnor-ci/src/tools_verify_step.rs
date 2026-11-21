// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use velnor_model::{CacheKey, CachePath, Host, ToolsAnchor};

use crate::cache_version::{Compression, cache_version};
use crate::error::CiError;
use crate::escape::{escape, inert};
use crate::mbx_store::trusted_event;
use crate::tools_discard;
use crate::tools_verify::{Anchor, Discard, Recording, Verb, Verdict, check, read, record};

const ANCHOR_DIR: &str = "velnor-tools-anchor";
const ANCHOR_NAME: &str = "digest";
pub fn tools_paths() -> Result<Vec<&'static str>, CiError> {
    literal_paths(
        &CacheKey::Tools {
            host: Host,
            anchor: ToolsAnchor::Unanchored,
        }
        .paths(),
    )
}

pub fn literal_paths(paths: &[CachePath]) -> Result<Vec<&'static str>, CiError> {
    paths
        .iter()
        .map(|path| {
            path.literal()
                .ok_or_else(|| CiError::new("a tools cache path is not a literal directory"))
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Remote {
    pub host: String,
    pub api_url: String,
    pub repository: String,
}

#[derive(Debug, Clone)]
pub struct Context {
    pub home: PathBuf,
    pub runner_temp: PathBuf,
    pub github_output: Option<PathBuf>,
    pub anchor: String,
    pub anchor_matched: bool,
    pub verified: bool,
    pub event_name: String,
    pub git_ref: String,
    pub default_branch: Option<String>,
    pub cache_key: String,
    pub anchor_key: String,
    pub remote: Remote,
    pub compression: Compression,
}

#[derive(Debug, Clone, Copy)]
pub struct Remedy<'a> {
    pub key: &'a str,
    pub key_version: &'a str,
    pub anchor_key: &'a str,
    pub anchor_version: &'a str,
    pub remote: &'a Remote,
    pub reference: Option<&'a str>,
}

fn reason(discard: &Discard) -> String {
    match discard {
        Discard::NoAnchor => "no anchor was restored for this key".to_owned(),
        Discard::BadAnchor => "the anchor is malformed".to_owned(),
        Discard::Mismatch => "they differ from the anchor".to_owned(),
        Discard::Unreadable(why) => inert(why),
        Discard::Untrusted => "this is not a trusted default-branch run".to_owned(),
    }
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn delete_command(key: &str, version: &str, remedy: Remedy<'_>) -> String {
    let remote = remedy.remote;
    let mut command = format!(
        "GITHUB_API_URL={} GITHUB_TOKEN=\"$(gh auth token --hostname {})\" velnor-ci cache delete-entry --repo {} --key {} --version {version}",
        quoted(&remote.api_url),
        quoted(&remote.host),
        quoted(&remote.repository),
        quoted(key),
    );
    if let Some(reference) = remedy.reference {
        command = format!("{command} --ref {}", quoted(reference));
    }
    command
}

fn delete_sentence(what: &str, key: &str, version: &str, remedy: Remedy<'_>) -> String {
    format!(
        ". The restored {what} is {} (version {version}); delete that entry by ID with `{}`, which deletes only the entry with that key and version and refuses when more than one entry matches",
        quoted(key),
        delete_command(key, version, remedy)
    )
}

fn anchor_sentence(remedy: Remedy<'_>) -> String {
    if remedy.anchor_key.is_empty() {
        return String::new();
    }
    delete_sentence(
        "anchor entry",
        remedy.anchor_key,
        remedy.anchor_version,
        remedy,
    )
}

fn delete_hint(remedy: Remedy<'_>) -> String {
    let mut hint = String::new();
    if !remedy.key.is_empty() {
        hint.push_str(&delete_sentence(
            "Actions cache entry",
            remedy.key,
            remedy.key_version,
            remedy,
        ));
    }
    hint.push_str(&anchor_sentence(remedy));
    escape(&hint, false)
}

fn anchor_hint(remedy: Remedy<'_>) -> String {
    escape(&anchor_sentence(remedy), false)
}

fn host_of(server: Option<&str>) -> &str {
    server
        .map(|url| {
            url.trim_end_matches('/')
                .trim_start_matches("https://")
                .trim_start_matches("http://")
        })
        .filter(|host| !host.is_empty())
        .unwrap_or("github.com")
}

fn remote(server: Option<&str>, api: Option<&str>, repository: Option<&str>) -> Remote {
    let host = host_of(server);
    let api_url = api.filter(|url| !url.is_empty()).map_or_else(
        || {
            if host == "github.com" {
                "https://api.github.com".to_owned()
            } else {
                format!("https://{host}/api/v3")
            }
        },
        str::to_owned,
    );
    Remote {
        host: host.to_owned(),
        api_url,
        repository: repository.unwrap_or("<owner/repo>").to_owned(),
    }
}

#[must_use]
pub fn message(verdict: &Verdict, remedy: Remedy<'_>) -> String {
    match verdict {
        Verdict::Anchored(digest) => format!("velnor: the tools anchor is {digest}"),
        Verdict::Unanchored(why) => {
            format!("velnor: there is no usable tools anchor ({})", reason(why))
        }
        Verdict::NothingRestored => {
            "velnor: no tool directories are present, so none are verified".to_owned()
        }
        Verdict::Verified => "velnor: the restored tools equal the anchor".to_owned(),
        Verdict::Discarded(why) => {
            let hint = match why {
                Discard::Mismatch => delete_hint(remedy),
                Discard::BadAnchor => anchor_hint(remedy),
                _ => String::new(),
            };
            format!(
                "::notice::the restored tools were discarded ({}); they install from scratch{hint}",
                reason(why)
            )
        }
        Verdict::Recorded(_) => "velnor: recorded the tools anchor".to_owned(),
        Verdict::Matches(_) => {
            "velnor: the tools equal the anchor, so they may be saved".to_owned()
        }
        Verdict::Differs(why) => format!("::notice::the tools are not saved ({})", reason(why)),
    }
}

fn outputs(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Anchored(digest) => format!("anchor={digest}\n"),
        Verdict::Unanchored(_) => "anchor=\n".to_owned(),
        Verdict::Verified => "verified=true\n".to_owned(),
        Verdict::NothingRestored | Verdict::Discarded(_) => "verified=false\n".to_owned(),
        Verdict::Recorded(digest) => format!("matches=true\nrecorded=true\ndigest={digest}\n"),
        Verdict::Matches(digest) => format!("matches=true\nrecorded=false\ndigest={digest}\n"),
        Verdict::Differs(_) => "matches=false\nrecorded=false\n".to_owned(),
    }
}

fn checked(context: &Context, anchor: &Anchor<'_>, file: &Path) -> Result<Verdict, CiError> {
    let verdict = check(&context.home, anchor, file)?;
    if matches!(
        verdict,
        Verdict::Discarded(Discard::Mismatch | Discard::Unreadable(_))
    ) {
        tools_discard::mark(&context.runner_temp)
            .map_err(|error| CiError::new(format!("{error}; the poisoned entry would be kept")))?;
    }
    Ok(verdict)
}

pub fn run(verb: Verb, context: &Context) -> Result<String, CiError> {
    let file = context.runner_temp.join(ANCHOR_DIR).join(ANCHOR_NAME);
    let anchor = Anchor(&context.anchor);
    let verdict = match verb {
        Verb::Read => read(&file, context.anchor_matched),
        Verb::Check => checked(context, &anchor, &file)?,
        Verb::Record => record(
            &context.home,
            &Recording {
                anchor,
                file: &file,
                trusted: trusted_event(
                    &context.event_name,
                    &context.git_ref,
                    context.default_branch.as_deref(),
                ),
                verified: context.verified,
            },
        )?,
    };
    let path = context
        .github_output
        .as_ref()
        .ok_or_else(|| CiError::new("set GITHUB_OUTPUT"))?;
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut handle| handle.write_all(outputs(&verdict).as_bytes()))
        .map_err(|error| CiError::new(format!("cannot write {}: {error}", path.display())))?;
    let anchor_dir = context.runner_temp.join(ANCHOR_DIR);
    let key_version = cache_version(&tools_paths()?, context.compression);
    let anchor_version = cache_version(&[&anchor_dir.to_string_lossy()], context.compression);
    let reference = context
        .default_branch
        .as_ref()
        .map(|branch| format!("refs/heads/{branch}"));
    let remedy = Remedy {
        key: &context.cache_key,
        key_version: &key_version,
        anchor_key: &context.anchor_key,
        anchor_version: &anchor_version,
        remote: &context.remote,
        reference: reference.as_deref(),
    };
    Ok(format!("{}\n", message(&verdict, remedy)))
}

pub fn setup(verb: Verb) -> Result<String, CiError> {
    let var = |name: &str| std::env::var(name).ok();
    let required = |name: &str| {
        var(name)
            .map(PathBuf::from)
            .ok_or_else(|| CiError::new(format!("set {name}")))
    };
    let default_branch = var("GITHUB_EVENT_PATH")
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| crate::event::parse_event(&text).ok())
        .and_then(|event| event.default_branch);
    let home = required("HOME")?;
    run(
        verb,
        &Context {
            compression: Compression::detect(&home),
            home,
            runner_temp: tools_discard::runner_temp(var("RUNNER_TEMP")).ok_or_else(|| {
                CiError::new("set RUNNER_TEMP to an absolute path; the tools anchor lives under it")
            })?,
            github_output: var("GITHUB_OUTPUT").map(PathBuf::from),
            anchor: var("VELNOR_TOOLS_ANCHOR").unwrap_or_default(),
            anchor_matched: var("VELNOR_ANCHOR_MATCHED").is_some_and(|key| !key.is_empty()),
            verified: var("VELNOR_TOOLS_VERIFIED").as_deref() == Some("true"),
            event_name: var("GITHUB_EVENT_NAME").unwrap_or_default(),
            git_ref: var("GITHUB_REF").unwrap_or_default(),
            default_branch,
            cache_key: var("VELNOR_TOOLS_CACHE_KEY").unwrap_or_default(),
            anchor_key: var("VELNOR_ANCHOR_CACHE_KEY").unwrap_or_default(),
            remote: remote(
                var("GITHUB_SERVER_URL").as_deref(),
                var("GITHUB_API_URL").as_deref(),
                var("GITHUB_REPOSITORY").as_deref(),
            ),
        },
    )
}
