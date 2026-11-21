// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::CiError;
use crate::escape::{escape, inert, shell_quote as quoted};
use crate::io::cache_delete_entry::ref_options;
use crate::mbx_store::trusted_event;
use crate::restore_clock;
use crate::tools_discard;
use crate::tools_verify::{
    Anchor, Discard, MALFORMED_ANCHOR, Recording, Verb, Verdict, check, read, record,
};

const ANCHOR_DIR: &str = "velnor-tools-anchor";
const ANCHOR_NAME: &str = "digest";

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
    pub base_ref: String,
    pub default_branch: Option<String>,
    pub cache_key: String,
    pub restored_before: String,
    pub remote: Remote,
}

#[derive(Debug, Clone, Copy)]
pub struct Remedy<'a> {
    pub key: &'a str,
    pub remote: &'a Remote,
    pub refs: &'a [String],
    pub restored_before: &'a str,
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

const BAD_ANCHOR_HINT: &str = ". The next trusted run on the default branch records a new anchor, and the newest anchor wins, so nothing needs deleting";

fn delete_command(key: &str, refs: &[String], remote: &Remote, restored_before: &str) -> String {
    let refs = ref_options(refs);
    format!(
        "GITHUB_API_URL={} GITHUB_TOKEN=\"$(gh auth token --hostname {})\" velnor-ci cache delete-entry --repo {} --key {}{refs} --restored-before {}",
        quoted(&remote.api_url),
        quoted(&remote.host),
        quoted(&remote.repository),
        quoted(key),
        quoted(restored_before),
    )
}

const HELPER_RELEASES: &str = "https://github.com/tailrocks/velnor-new/releases";

fn trust_hint(remote: &Remote) -> String {
    if remote.host == "github.com" {
        return String::new();
    }
    format!(
        " If the certificate of {} is signed by a private CA, set SSL_CERT_FILE to a PEM bundle that holds it",
        quoted(&remote.host)
    )
}

fn delete_sentence(what: &str, key: &str, remedy: Remedy<'_>) -> String {
    if remedy.refs.is_empty() {
        return String::new();
    }
    format!(
        ". The restored {what} is {}; the cache action does not report which ref or version it came from, so the command below only reads: it lists every ref the action can have read, in its order (the ref of this run, then the base and default branches), and prints the one entry the restore read (ID, version and ref) together with the command that deletes that entry by its ID. It deletes nothing itself, so running it again is harmless. It resolves an entry only when the FIRST ref holds the key and the entry was saved before --restored-before (the time this job's restore began, taken from the record that `tools-local clear` leaves before the restores or, when there is none, from this check, whichever is earlier, minus a safety margin of {margin} minutes for clock skew; an entry saved at or after it, or without a usable created_at, is not shown to be the entry this run read, so a stale notice of another run does not resolve an entry saved after this run's restore, unless the runner clock is ahead of GitHub's by more than the margin; a refusal says why and prints the command that reruns step 1 with an explicit --restored-before), and refuses, printing nothing to delete, when the first ref no longer holds it (another job of this run or you may have deleted the entry already; if so, stop), when that ref has more than one entry, when a later ref holds the key under a different version or when an entry has no version; then check which version the job used and delete that ID. Step 1: run the command below and read the entry it prints. Step 2: run the --id command it prints, once; it deletes exactly that ID and refuses when the ID no longer exists or has another key. The two steps are needed once per run, not once per job, even though every tools job of the run prints this notice: after one job's entry is deleted, step 1 refuses instead of naming the next ref's healthy entry. It needs velnor-ci 0.1.112 or newer on the machine that runs it, which the pinned helper does not provide: download it from {HELPER_RELEASES} (tag tools-v0.1.112 or newer) and put it on PATH. The token is the one `gh auth token` prints, which prefers an exported GH_TOKEN or GITHUB_TOKEN (GH_ENTERPRISE_TOKEN or GITHUB_ENTERPRISE_TOKEN on a host other than github.com) to the stored login, and it needs actions: write.{} Command: `{}`",
        quoted(key),
        trust_hint(remedy.remote),
        delete_command(key, remedy.refs, remedy.remote, remedy.restored_before),
        margin = restore_clock::SAFETY_MARGIN_SECONDS / 60,
    )
}

fn delete_hint(remedy: Remedy<'_>) -> String {
    if remedy.key.is_empty() {
        return String::new();
    }
    escape(
        &delete_sentence("Actions cache entry", remedy.key, remedy),
        false,
    )
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
                Discard::BadAnchor => BAD_ANCHOR_HINT.to_owned(),
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
        Verdict::Unanchored(Discard::BadAnchor) => format!("anchor={MALFORMED_ANCHOR}\n"),
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

fn restore_refs(context: &Context) -> Vec<String> {
    let branch = |name: Option<&str>| {
        name.filter(|name| !name.is_empty())
            .map(|name| format!("refs/heads/{name}"))
    };
    let mut refs: Vec<String> = Vec::new();
    for candidate in [
        Some(context.git_ref.clone()),
        branch(Some(&context.base_ref)),
        branch(context.default_branch.as_deref()),
    ]
    .into_iter()
    .flatten()
    {
        if !candidate.is_empty() && !refs.contains(&candidate) {
            refs.push(candidate);
        }
    }
    refs
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
    let refs = restore_refs(context);
    let remedy = Remedy {
        key: &context.cache_key,
        remote: &context.remote,
        refs: &refs,
        restored_before: &context.restored_before,
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
    let runner_temp = tools_discard::runner_temp(var("RUNNER_TEMP")).ok_or_else(|| {
        CiError::new("set RUNNER_TEMP to an absolute path; the tools anchor lives under it")
    })?;
    let restored_before = restore_clock::cutoff(&runner_temp, crate::time::now_unix());
    run(
        verb,
        &Context {
            home: required("HOME")?,
            runner_temp,
            github_output: var("GITHUB_OUTPUT").map(PathBuf::from),
            anchor: var("VELNOR_TOOLS_ANCHOR").unwrap_or_default(),
            anchor_matched: var("VELNOR_ANCHOR_MATCHED").is_some_and(|key| !key.is_empty()),
            verified: var("VELNOR_TOOLS_VERIFIED").as_deref() == Some("true"),
            event_name: var("GITHUB_EVENT_NAME").unwrap_or_default(),
            git_ref: var("GITHUB_REF").unwrap_or_default(),
            base_ref: var("GITHUB_BASE_REF").unwrap_or_default(),
            default_branch,
            cache_key: var("VELNOR_TOOLS_CACHE_KEY").unwrap_or_default(),
            restored_before,
            remote: remote(
                var("GITHUB_SERVER_URL").as_deref(),
                var("GITHUB_API_URL").as_deref(),
                var("GITHUB_REPOSITORY").as_deref(),
            ),
        },
    )
}
