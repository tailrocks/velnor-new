// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use super::json::{Res, is_hex, require};
use crate::error::CiError;

/// The most the sealed executable may hold, and the most of it that is read.
pub const MAX_EXECUTABLE: u64 = 268_435_456;

/// The variables the Mac proof program may see. Nothing else crosses the exec.
pub const ALLOWED_ENV: [&str; 21] = [
    "PATH",
    "HOME",
    "RUNNER_TEMP",
    "GH_TOKEN",
    "GITHUB_SHA",
    "GITHUB_RUN_ID",
    "GITHUB_RUN_ATTEMPT",
    "GITHUB_JOB",
    "GITHUB_OUTPUT",
    "GITHUB_EVENT_NAME",
    "GITHUB_REF",
    "GITHUB_REF_PROTECTED",
    "GITHUB_REPOSITORY",
    "GITHUB_WORKFLOW_REF",
    "GITHUB_WORKFLOW_SHA",
    "VELNOR_Q_REPOSITORY",
    "VELNOR_Q_PATH",
    "VELNOR_Q_REF",
    "VELNOR_Q_SHA",
    "VELNOR_Q_CHECK_RUN_ID",
    "VELNOR_Q_GH",
];

/// What `verify-mac-proof` runs: the verified Linux SDK, sealed in memory, with these
/// arguments and this environment.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub source: PathBuf,
    pub expected_sha256: String,
    pub args: Vec<OsString>,
    pub env: BTreeMap<String, String>,
}

/// Port of the argument and environment handling of `source_preview_run_verified.py`.
/// `vars` is the process environment.
pub fn plan(vars: &BTreeMap<String, String>) -> Res<Plan> {
    let get = |key: &str| {
        vars.get(key)
            .ok_or_else(|| CiError::new("source preview executable rejected"))
    };
    let root = PathBuf::from(get("RUNNER_TEMP")?);
    let expected = get("VERIFIED_LINUX_SHA256")?.clone();
    require(root.is_absolute() && is_hex(&expected, 64))?;
    let mut args: Vec<OsString> = vec!["image-maintenance".into(), "verify-mac-attestation".into()];
    for (flag, key) in [
        ("--producer-run-id", "PRODUCER_RUN_ID"),
        ("--producer-run-attempt", "PRODUCER_RUN_ATTEMPT"),
        ("--producer-artifact-id", "PRODUCER_ARTIFACT_ID"),
        ("--publisher-artifact-id", "PUBLISHER_ARTIFACT_ID"),
    ] {
        args.push(flag.into());
        args.push(get(key)?.into());
    }
    args.push("--assets-dir".into());
    args.push(root.join("runner-family-release").join("assets").into());
    args.push("--runner-temp".into());
    args.push(root.clone().into());
    let env = vars
        .iter()
        .filter(|(key, _)| ALLOWED_ENV.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Ok(Plan {
        source: root.join("source-preview-runtime").join("linux"),
        expected_sha256: expected,
        args,
        env,
    })
}
