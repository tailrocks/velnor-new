// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BaseExpiry, CacheKey, JobId, RepoPath, Step, StepKind, TrimFamily, Visibility};

use crate::cache::render_key;
use crate::gate::{BASE_STEP, visibility_part};
use crate::release_steps::shell_word;
use crate::rules::{RenderedStep, Value};

const BASE_SAVE_OUTPUT: &str = "base_save";
const BASE_PREFIX_OUTPUT: &str = "base_prefix";
const SOURCES_PREFIX_OUTPUT: &str = "sources_prefix";
const MAX_BASE_AGE_DAYS: u64 = 7;
const BASE_AGE_VARIABLE: &str = "VELNOR_BASE_MAX_AGE_DAYS";
const DEFAULT_BRANCH_REF: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

pub(crate) fn cache_decide(target: &RepoPath, archive: Option<&RepoPath>) -> RenderedStep {
    RenderedStep {
        id: Some(BASE_STEP.to_owned()),
        name: "Decide whether to save the build directory".to_owned(),
        run: Some(format!(
            "velnor-ci cache decide --target-dir {}{}",
            shell_word(target.as_str()),
            archive.map_or_else(String::new, |file| format!(
                " --archive {}",
                shell_word(file.as_str())
            ))
        )),
        ..RenderedStep::default()
    }
}

pub(crate) fn lane_outputs(steps: &[Step]) -> Vec<(String, String)> {
    let prefix = |wanted: fn(&CacheKey) -> bool| {
        steps.iter().find_map(|step| match &step.kind {
            StepKind::CacheSave(key) if wanted(key) => render_key(key).restore_keys.pop(),
            _ => None,
        })
    };
    let decides = steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::CacheDecide { .. }));
    let target = prefix(|key| matches!(key, CacheKey::Target { .. })).filter(|_| decides);
    let sources = prefix(|key| matches!(key, CacheKey::Sources { .. })).filter(|_| decides);
    target
        .map(|prefix| {
            [
                (BASE_PREFIX_OUTPUT.to_owned(), prefix),
                (
                    BASE_SAVE_OUTPUT.to_owned(),
                    format!("${{{{ steps.{BASE_STEP}.outputs.save }}}}"),
                ),
            ]
        })
        .into_iter()
        .flatten()
        .chain(sources.map(|prefix| (SOURCES_PREFIX_OUTPUT.to_owned(), prefix)))
        .collect()
}

pub(crate) fn trimmed_lane(lane: &JobId, family: TrimFamily) -> String {
    match family {
        TrimFamily::Target => format!("needs.{lane}.outputs.{BASE_SAVE_OUTPUT} == 'true'"),
        TrimFamily::Sources => format!("needs.{lane}.outputs.{SOURCES_PREFIX_OUTPUT} != ''"),
    }
}

pub(crate) fn cache_trim(lane: &JobId, family: TrimFamily) -> RenderedStep {
    let (name, output) = match family {
        TrimFamily::Target => (
            format!("Delete the older {lane} entries"),
            BASE_PREFIX_OUTPUT,
        ),
        TrimFamily::Sources => (
            format!("Delete the older {lane} sources entries"),
            SOURCES_PREFIX_OUTPUT,
        ),
    };
    RenderedStep {
        name,
        env: vec![
            (
                "CACHE_PREFIX".to_owned(),
                Value::Expr(format!("${{{{ needs.{lane}.outputs.{output} }}}}")),
            ),
            (
                "GITHUB_TOKEN".to_owned(),
                Value::Expr("${{ github.token }}".to_owned()),
            ),
        ],
        run: Some("velnor-ci cache trim --prefix \"$CACHE_PREFIX\"".to_owned()),
        ..RenderedStep::default()
    }
}

pub(crate) fn skipped_base_saves() -> RenderedStep {
    RenderedStep {
        name: "Explain the skipped base cache saves".to_owned(),
        run: Some(
            "echo \"::notice::Base cache saves skipped: this repository is public and has private or non-crates.io dependencies, so a cache built from them must not be readable by forks\"".to_owned(),
        ),
        ..RenderedStep::default()
    }
}

pub(crate) fn base_age(expiry: BaseExpiry, visibility: Visibility) -> Vec<(String, Value)> {
    let on = match expiry {
        BaseExpiry::Never => return Vec::new(),
        BaseExpiry::OnDefaultBranch => DEFAULT_BRANCH_REF.to_owned(),
        BaseExpiry::OnDispatch => {
            format!("{DEFAULT_BRANCH_REF} && github.event_name == 'workflow_dispatch'")
        }
        BaseExpiry::OnDefaultBranchExceptPush => {
            format!("{DEFAULT_BRANCH_REF} && github.event_name != 'push'")
        }
    };
    let on = visibility_part(visibility).map_or(on.clone(), |part| format!("{on} && {part}"));
    vec![(
        BASE_AGE_VARIABLE.to_owned(),
        Value::Expr(format!("${{{{ {on} && '{MAX_BASE_AGE_DAYS}' || '0' }}}}")),
    )]
}

pub(crate) fn cache_budget(bytes: u64) -> RenderedStep {
    RenderedStep {
        name: "Enforce the repository cache budget".to_owned(),
        env: vec![(
            "GITHUB_TOKEN".to_owned(),
            Value::Expr("${{ github.token }}".to_owned()),
        )],
        run: Some(format!("velnor-ci cache budget --budget-bytes {bytes}")),
        ..RenderedStep::default()
    }
}
