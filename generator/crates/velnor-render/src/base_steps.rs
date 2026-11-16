// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BaseExpiry, CacheKey, JobId, RepoPath, Step, StepKind};

use crate::cache::render_key;
use crate::gate::BASE_STEP;
use crate::release_steps::shell_word;
use crate::rules::{RenderedStep, Value};

const BASE_SAVE_OUTPUT: &str = "base_save";
const BASE_PREFIX_OUTPUT: &str = "base_prefix";
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
    let prefix = steps.iter().find_map(|step| match &step.kind {
        StepKind::CacheSave(key @ CacheKey::Target { .. }) => render_key(key).restore_keys.pop(),
        _ => None,
    });
    let decides = steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::CacheDecide { .. }));
    prefix
        .filter(|_| decides)
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
        .collect()
}

pub(crate) fn trimmed_lane(lane: &JobId) -> String {
    format!("needs.{lane}.outputs.{BASE_SAVE_OUTPUT} == 'true'")
}

pub(crate) fn cache_trim(lane: &JobId) -> RenderedStep {
    RenderedStep {
        name: format!("Delete the older {lane} entries"),
        env: vec![
            (
                "CACHE_PREFIX".to_owned(),
                Value::Expr(format!(
                    "${{{{ needs.{lane}.outputs.{BASE_PREFIX_OUTPUT} }}}}"
                )),
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

pub(crate) fn base_age(expiry: BaseExpiry) -> Vec<(String, Value)> {
    let on = match expiry {
        BaseExpiry::Never => return Vec::new(),
        BaseExpiry::OnDefaultBranch => DEFAULT_BRANCH_REF.to_owned(),
        BaseExpiry::OnDefaultBranchExceptPush => {
            format!("{DEFAULT_BRANCH_REF} && github.event_name != 'push'")
        }
    };
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
