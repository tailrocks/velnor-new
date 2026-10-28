// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::{Value, json};

use super::FixtureProfile;

#[derive(Clone, Copy)]
pub(crate) enum CapturedOutcome {
    Missing,
    Unauthorized,
    Transport,
}

#[derive(Clone, Copy, Default)]
pub(crate) enum ReplayMutation {
    #[default]
    Exact,
    WrongExit,
    NonemptyStdout,
    WrongReference,
    WrongUrl,
    ExtraLine,
}

struct Capture {
    code: u8,
    stdout: &'static str,
    stderr: &'static str,
    reference: &'static str,
    manifest_url: &'static str,
}

pub(crate) fn replay(
    outcome: CapturedOutcome,
    mutation: ReplayMutation,
    profile: FixtureProfile,
    source: &str,
) -> Result<Value, Box<dyn Error>> {
    let capture = capture(outcome);
    if capture.stderr.matches(capture.manifest_url).count() != 1
        || capture.stderr.matches(capture.reference).count() != 1
    {
        return Err("captured Crane manifest output has an ambiguous URL or reference".into());
    }
    let reference = format!("{}:sha-{source}", profile.repository());
    let image = profile
        .repository()
        .strip_prefix("ghcr.io/")
        .ok_or("fixture repository is not in GHCR")?;
    let (_, tag) = reference
        .rsplit_once(':')
        .ok_or("fixture tag reference is missing")?;
    let mut manifest_url = format!("https://ghcr.io/v2/{image}/manifests/{tag}");
    let mut code = capture.code;
    let mut stdout = capture.stdout.to_owned();
    let mut stderr = capture.stderr.to_owned();
    match mutation {
        ReplayMutation::Exact => {}
        ReplayMutation::WrongExit => code = 2,
        ReplayMutation::NonemptyStdout => "unexpected stdout\n".clone_into(&mut stdout),
        ReplayMutation::WrongReference => {
            stderr = stderr.replace(capture.reference, &format!("{reference}-wrong"));
        }
        ReplayMutation::WrongUrl => manifest_url.push_str("-wrong"),
        ReplayMutation::ExtraLine => stderr.push_str("unexpected extra line\n"),
    }
    if !matches!(mutation, ReplayMutation::WrongReference) {
        stderr = stderr.replace(capture.reference, &reference);
    }
    stderr = stderr.replace(capture.manifest_url, &manifest_url);
    Ok(json!({"code":code,"stdout":stdout,"stderr":stderr}))
}

fn capture(outcome: CapturedOutcome) -> Capture {
    match outcome {
        CapturedOutcome::Missing => Capture {
            code: 1,
            stdout: "",
            stderr: "Error: fetching manifest 127.0.0.1:62219/fixture/image:missing: GET http://127.0.0.1:62219/v2/fixture/image/manifests/missing: MANIFEST_UNKNOWN: manifest unknown\n",
            reference: "127.0.0.1:62219/fixture/image:missing",
            manifest_url: "http://127.0.0.1:62219/v2/fixture/image/manifests/missing",
        },
        CapturedOutcome::Unauthorized => Capture {
            code: 1,
            stdout: "",
            stderr: "Error: fetching manifest 127.0.0.1:62223/fixture/image:unauthorized: GET http://127.0.0.1:62223/v2/fixture/image/manifests/unauthorized: UNAUTHORIZED: fixture auth required\n",
            reference: "127.0.0.1:62223/fixture/image:unauthorized",
            manifest_url: "http://127.0.0.1:62223/v2/fixture/image/manifests/unauthorized",
        },
        CapturedOutcome::Transport => Capture {
            code: 1,
            stdout: "",
            stderr: "2026/10/28 09:55:40 retrying EOF\n2026/10/28 09:55:41 retrying EOF\n2026/10/28 09:55:44 retrying EOF\nError: fetching manifest 127.0.0.1:62227/fixture/image:transport: Get \"http://127.0.0.1:62227/v2/fixture/image/manifests/transport\": EOF\n",
            reference: "127.0.0.1:62227/fixture/image:transport",
            manifest_url: "http://127.0.0.1:62227/v2/fixture/image/manifests/transport",
        },
    }
}
