// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BaseExpiry, CacheKey, RepoPath};

use crate::cache::render_key;
use crate::gate::BASE_STEP;
use crate::release_steps::shell_word;
use crate::rules::{RenderedStep, Value};

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

pub(crate) fn cache_trim(key: &CacheKey) -> RenderedStep {
    RenderedStep {
        name: format!("Delete the older {} entries", key.family()),
        env: vec![
            (
                "CACHE_PREFIX".to_owned(),
                Value::Expr(
                    render_key(key)
                        .restore_keys
                        .last()
                        .cloned()
                        .unwrap_or_default(),
                ),
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
