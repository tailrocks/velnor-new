// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{CheckoutScope, FullRun, OnMain, RepoPath, RepoView};
use velnor_rust::{
    bytes_outside_lanes, detect, silent_build_scripts, suggest_groups as suggested_groups,
};

use crate::{GenerateError, SPARSE_HINT_HELPER};

pub const NO_FULL_RUN_NOTE: &str = "on_main is \"affected\" and full_run is \"off\", so no run ever selects everything and a gap in selection would go unnoticed";

pub fn notes(view: &RepoView) -> Result<Vec<String>, GenerateError> {
    let config = detect(view)?.config;
    let blind = config.on_main == OnMain::Affected && config.full_run == FullRun::Off;
    Ok(blind
        .then(|| NO_FULL_RUN_NOTE.to_owned())
        .into_iter()
        .chain(
            silent_build_scripts(view)
                .iter()
                .map(velnor_rust::SilentBuildScript::note),
        )
        .collect())
}

pub const SPARSE_SUGGESTION_BYTES: u64 = 100 * 1024 * 1024;

pub fn sparse_note(
    view: &RepoView,
    sizes: &BTreeMap<RepoPath, u64>,
) -> Result<Option<String>, GenerateError> {
    let facts = detect(view)?;
    if facts.config.checkout == CheckoutScope::Sparse {
        return Ok(None);
    }
    Ok(bytes_outside_lanes(&facts, sizes)
        .filter(|bytes| *bytes > SPARSE_SUGGESTION_BYTES)
        .map(|bytes| {
            format!(
                "{} MiB of tracked files lie outside the paths Rust jobs read; set sparse_checkout = true in .velnor/rust-ci.toml so each job checks out only what it reads (needs velnor-ci {}.{}.{} or newer to name the [[inputs]] line a test that reads an unlisted file needs; see CONFIG.md)",
                bytes / (1024 * 1024),
                SPARSE_HINT_HELPER.0,
                SPARSE_HINT_HELPER.1,
                SPARSE_HINT_HELPER.2
            )
        }))
}

pub fn suggest_groups(view: &RepoView) -> Result<(String, Vec<String>), GenerateError> {
    let facts = detect(view)?;
    Ok((suggested_groups(&facts), facts.timings_notes))
}
