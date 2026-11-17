// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BaseExpiry, CacheKey, HelperCommand, JobId, RepoPath, Step, StepKind, TrimFamily,
};

use crate::cache::render_key;
use crate::cache_upload::SOURCES_KEY_OUTPUT;
use crate::gate::BASE_STEP;
use crate::gate::sources_save_step;
use crate::release_steps::shell_word;
use crate::rules::{MAX_RUN_CHARACTERS, RenderedStep, Value};

const BASE_SAVE_OUTPUT: &str = "base_save";
const BASE_PREFIX_OUTPUT: &str = "base_prefix";
const SOURCES_PREFIX_OUTPUT: &str = "sources_prefix";
const SOURCES_SAVED_OUTPUT: &str = "sources_saved";
const MAX_BASE_AGE_DAYS: u64 = 7;
const BASE_AGE_VARIABLE: &str = "VELNOR_BASE_MAX_AGE_DAYS";
const DEFAULT_BRANCH_REF: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

pub(crate) fn cache_decide(
    target: &RepoPath,
    archive: Option<&RepoPath>,
    manifests: &[RepoPath],
) -> RenderedStep {
    let mut rendered = RenderedStep {
        condition: Some("${{ steps.inputs_save.outcome == 'success' }}".to_owned()),
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
    };
    if target.as_str().contains("${{")
        || archive.is_some_and(|path| path.as_str().contains("${{"))
        || rendered
            .run
            .as_ref()
            .is_some_and(|run| run.chars().count() > MAX_RUN_CHARACTERS)
    {
        let target = crate::helper_steps::hex_path(target.as_str());
        let archive = archive.map_or_else(
            || "null".to_owned(),
            |path| format!("\"{}\"", crate::helper_steps::hex_path(path.as_str())),
        );
        rendered.env.push((
            "VELNOR_CACHE_PATHS".to_owned(),
            Value::Text(format!("[\"{target}\",{archive}]")),
        ));
        rendered.run = Some("velnor-ci cache decide --paths-env VELNOR_CACHE_PATHS".to_owned());
    }
    rendered
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
    let saved = steps.iter().find_map(|step| match &step.kind {
        StepKind::CacheSave(CacheKey::Sources { root, .. }) if decides => {
            Some(sources_save_step(root.slug()))
        }
        _ => None,
    });
    let keys = [TrimFamily::Target, TrimFamily::Sources]
        .into_iter()
        .filter_map(|family| {
            steps.iter().find_map(|step| match &step.kind {
                StepKind::CacheSave(key) => crate::cache_upload::key_id(key)
                    .filter(|_| crate::cache_upload::family_of(key) == Some(family))
                    .map(|id| {
                        (
                            crate::cache_upload::key_output(family).to_owned(),
                            format!("${{{{ steps.{id}.outputs.key }}}}"),
                        )
                    }),
                _ => None,
            })
        });
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
        .chain(keys)
        .chain(sources.map(|prefix| (SOURCES_PREFIX_OUTPUT.to_owned(), prefix)))
        .chain(saved.map(|id| {
            (
                SOURCES_SAVED_OUTPUT.to_owned(),
                format!("${{{{ steps.{id}.outcome == 'success' }}}}"),
            )
        }))
        .collect()
}

pub(crate) fn with_saved_lane(condition: String, steps: &[Step]) -> String {
    let mut terms = Vec::new();
    for step in steps {
        if let StepKind::CacheTrim { lane, family } = &step.kind {
            let term = admitted_lane(lane, *family);
            if !terms.contains(&term) {
                terms.push(term);
            }
        }
    }
    if terms.is_empty() {
        return condition;
    }
    let clause = format!("({})", terms.join(" || "));
    let Some(inner) = condition
        .strip_prefix("${{ ")
        .and_then(|rest| rest.strip_suffix(" }}"))
    else {
        return format!("{condition} && {clause}");
    };
    format!("${{{{ {inner} && {clause} }}}}")
}

fn admitted_lane(lane: &JobId, family: TrimFamily) -> String {
    match family {
        TrimFamily::Target => trimmed_lane(lane, family),
        TrimFamily::Sources => format!(
            "(needs.{lane}.outputs.{SOURCES_SAVED_OUTPUT} == 'true' || needs.{lane}.outputs.{SOURCES_KEY_OUTPUT} != '')"
        ),
    }
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
