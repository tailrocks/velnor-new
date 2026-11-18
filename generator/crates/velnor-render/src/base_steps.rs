// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BASE_PREFIX_OUTPUT, BASE_SAVE_OUTPUT, BaseExpiry, CacheKey, CacheLaneOutput, HelperCommand,
    JobId, RepoPath, SOURCES_PREFIX_OUTPUT, SOURCES_SAVED_OUTPUT, Step, StepKind, TrimFamily,
    cache_lane_outputs,
};

use crate::cache::render_key;
use crate::cache_upload::{SOURCES_KEY_OUTPUT, key_id};
use crate::gate::sources_save_step;
use crate::gate::{BASE_STEP, SOURCES_LOCKS_STEP};
use crate::release_steps::shell_word;
use crate::rules::{RenderedStep, Value};

const MAX_BASE_AGE_DAYS: u64 = 7;
const BASE_AGE_VARIABLE: &str = "VELNOR_BASE_MAX_AGE_DAYS";
const DEFAULT_BRANCH_REF: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

pub(crate) fn cache_decide(
    target: &RepoPath,
    archive: Option<&RepoPath>,
    manifests: &[RepoPath],
) -> RenderedStep {
    RenderedStep {
        id: Some(BASE_STEP.to_owned()),
        name: "Decide whether to save the build directory".to_owned(),
        env: vec![(
            "VELNOR_CACHE_MANIFESTS".to_owned(),
            Value::Text(
                manifests
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        )],
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

pub(crate) fn sources_locks(locks: &[RepoPath]) -> RenderedStep {
    RenderedStep {
        id: Some(SOURCES_LOCKS_STEP.to_owned()),
        name: "Hash the sources lockfiles".to_owned(),
        env: vec![(
            "SOURCES_LOCKS".to_owned(),
            Value::Expr(crate::cache::hash_files(locks)),
        )],
        run: Some("echo \"hash=$SOURCES_LOCKS\" >> \"$GITHUB_OUTPUT\"".to_owned()),
        ..RenderedStep::default()
    }
}

pub(crate) fn lane_outputs(steps: &[Step]) -> Vec<(String, String)> {
    cache_lane_outputs(steps)
        .into_iter()
        .filter_map(|output| {
            let value = match &output {
                CacheLaneOutput::BasePrefix(key) | CacheLaneOutput::SourcesPrefix(key) => {
                    render_key(key).restore_keys.pop()?
                }
                CacheLaneOutput::BaseSave => format!("${{{{ steps.{BASE_STEP}.outputs.save }}}}"),
                CacheLaneOutput::SourcesSaved(CacheKey::Sources { lane, .. }) => format!(
                    "${{{{ steps.{}.outcome == 'success' }}}}",
                    sources_save_step(lane.slug())
                ),
                CacheLaneOutput::SourcesSaved(_) => return None,
                CacheLaneOutput::SavedKey(key) => {
                    format!("${{{{ steps.{}.outputs.key }}}}", key_id(key)?)
                }
            };
            Some((output.name().to_owned(), value))
        })
        .collect()
}

pub(crate) fn trimmed_lane(lane: &JobId, family: TrimFamily) -> String {
    match family {
        TrimFamily::Target => format!("needs.{lane}.outputs.{BASE_SAVE_OUTPUT} == 'true'"),
        TrimFamily::Sources => format!(
            "needs.{lane}.outputs.{SOURCES_PREFIX_OUTPUT} != '' && (needs.{lane}.outputs.{SOURCES_SAVED_OUTPUT} == 'true' || needs.{lane}.outputs.{SOURCES_KEY_OUTPUT} != '')"
        ),
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

pub(crate) fn base_age(expiry: BaseExpiry) -> Vec<(String, Value)> {
    match expiry {
        BaseExpiry::Never => Vec::new(),
        BaseExpiry::OnDefaultBranch => vec![(
            BASE_AGE_VARIABLE.to_owned(),
            Value::Expr(format!(
                "${{{{ {DEFAULT_BRANCH_REF} && '{MAX_BASE_AGE_DAYS}' || '0' }}}}"
            )),
        )],
    }
}

pub(crate) fn audited_lane(condition: String, job: &velnor_model::Job) -> String {
    if !job
        .steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::CacheDecide { .. }))
    {
        return condition;
    }
    if !job
        .needs
        .iter()
        .any(|id| id.as_str() == "rust-inputs-audit")
    {
        return condition;
    }
    let inner = condition.trim_start_matches("${{ ").trim_end_matches(" }}");
    format!(
        "${{{{ {inner} && (needs.rust-inputs-audit.result == 'success' || needs.rust-inputs-audit.result == 'skipped') }}}}"
    )
}

pub(crate) fn audited_save(
    job: &velnor_model::Job,
    step: &Step,
    mut bodies: Vec<RenderedStep>,
) -> Vec<RenderedStep> {
    let audited = job
        .needs
        .iter()
        .any(|id| id.as_str() == "rust-inputs-audit")
        && matches!(
            step.kind,
            StepKind::CacheDecide { .. } | StepKind::CacheSave(CacheKey::Target { .. })
        );
    let extra = audited.then(|| "${{ steps.restored.outputs.expired != 'true' || needs.rust-inputs-audit.result == 'success' }}".to_owned());
    for body in &mut bodies {
        body.condition = crate::gate::all_conditions(body.condition.take(), extra.clone());
    }
    bodies
}

pub(crate) fn job_setup(job: &velnor_model::Job) -> (bool, bool) {
    let has = |wanted: fn(&StepKind) -> bool| job.steps.iter().any(|step| wanted(&step.kind));
    let restores = has(|kind| {
        matches!(
            kind,
            StepKind::Helper(HelperCommand::Restored(..)) | StepKind::RestoredTargets { .. }
        )
    });
    (
        restores,
        has(|kind| *kind == StepKind::Helper(HelperCommand::InputsAudit)),
    )
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
