// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `velnor-ci resolve-qualification`: the predecessor a qualification dispatch names, checked
//! against the GitHub API. It ports the legacy `resolve-qualification-v1` op: the predecessor
//! comes from the dispatch inputs in the request, its run attempt is re-read through the API,
//! its receipt artifacts are walked, the source delta of a `useful_delta` run comes from git, and
//! the result is the bytes of `qualification-cache-admission.json`.

mod api;
mod archive;
mod chain;
mod delta;
mod request;
mod shape;
pub(crate) mod strict;
mod types;
mod validate;

use std::path::Path;

pub use api::Api;
pub use request::Request;

use crate::error::CiError;
use types::{Admission, Phase};

/// The admission file for the request, or `None` when the phase has no predecessor.
pub fn admission(
    request: &Request,
    root: &Path,
    api: &dyn Api,
) -> Result<Option<Vec<u8>>, CiError> {
    let context = request.context();
    let client = api::Client::for_repository(api, request.repository())?;
    client.validate_current_branch(&context.default_branch)?;
    let Some(predecessor) = context.predecessor else {
        return Ok(None);
    };
    let previous = chain::resolve(&client, predecessor, 1, None)?;
    let source_delta = if context.phase == Phase::UsefulDelta {
        Some(delta::derive(
            root,
            &previous.receipt.source_sha,
            &context.source_sha,
        )?)
    } else {
        None
    };
    let bytes = validate::canonical_bytes(&Admission {
        predecessor: previous,
        source_delta,
    })?;
    validate::parse_admission(&bytes)?;
    Ok(Some(bytes))
}

/// The one file the legacy op wrote, under `$RUNNER_TEMP`.
pub const ADMISSION_FILE: &str = "velnor/qualification-cache-admission.json";
