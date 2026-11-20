// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::error::CiError;
use crate::escape::inert;
use crate::mbx_store::trusted_event;
use crate::tools_verify::{Anchor, Discard, Recording, Verb, Verdict, check, read, record};

const ANCHOR_DIR: &str = "velnor-tools-anchor";
const ANCHOR_NAME: &str = "digest";

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

#[must_use]
pub fn message(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Anchored(digest) => format!("velnor: the tools anchor is {digest}"),
        Verdict::Unanchored(why) => {
            format!("velnor: there is no usable tools anchor ({})", reason(why))
        }
        Verdict::NothingRestored => {
            "velnor: no tool directories are present, so none are verified".to_owned()
        }
        Verdict::Verified => "velnor: the restored tools equal the anchor".to_owned(),
        Verdict::Discarded(why) => format!(
            "::notice::the restored tools were discarded ({}); they install from scratch",
            reason(why)
        ),
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

pub fn run(verb: Verb, context: &Context) -> Result<String, CiError> {
    let file = context.runner_temp.join(ANCHOR_DIR).join(ANCHOR_NAME);
    let anchor = Anchor(&context.anchor);
    let verdict = match verb {
        Verb::Read => read(&file, context.anchor_matched),
        Verb::Check => check(&context.home, &anchor)?,
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
    Ok(format!("{}\n", message(&verdict)))
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
    run(
        verb,
        &Context {
            home: required("HOME")?,
            runner_temp: required("RUNNER_TEMP")?,
            github_output: var("GITHUB_OUTPUT").map(PathBuf::from),
            anchor: var("VELNOR_TOOLS_ANCHOR").unwrap_or_default(),
            anchor_matched: var("VELNOR_ANCHOR_MATCHED").is_some_and(|key| !key.is_empty()),
            verified: var("VELNOR_TOOLS_VERIFIED").as_deref() == Some("true"),
            event_name: var("GITHUB_EVENT_NAME").unwrap_or_default(),
            git_ref: var("GITHUB_REF").unwrap_or_default(),
            default_branch,
        },
    )
}
