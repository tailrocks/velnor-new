// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BASE_PREFIX_OUTPUT, BASE_SAVE_OUTPUT, BaseExpiry, CacheKey, CacheLaneOutput, HelperCommand,
    InputsCli, JobId, PlanFrom, RepoPath, SOURCES_PREFIX_OUTPUT, SOURCES_SAVED_OUTPUT, Step, StepKind,
    TrimFamily, cache_lane_outputs,
};

use crate::cache::render_key;
use crate::cache_upload::{SOURCES_KEY_OUTPUT, key_id};
use crate::gate::sources_save_step;
use crate::gate::{BASE_STEP, SOURCES_LOCKS_STEP};
use crate::release_steps::shell_word;
use crate::rules::{MAX_RUN_CHARACTERS, RenderedStep, Value};

const MAX_BASE_AGE_DAYS: u64 = 7;
const BASE_AGE_VARIABLE: &str = "VELNOR_BASE_MAX_AGE_DAYS";
const DEFAULT_BRANCH_REF: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

pub(crate) fn cache_decide(
    target: &RepoPath,
    archive: Option<&RepoPath>,
    manifests: &[RepoPath],
    cli: InputsCli,
    budget_bytes: u64,
    verdicts: Option<&PlanFrom>,
) -> RenderedStep {
    let mut rendered = RenderedStep {
        id: Some(BASE_STEP.to_owned()),
        name: "Decide whether to save the build directory".to_owned(),
        env: vec![
            (
                "VELNOR_CACHE_MANIFESTS".to_owned(),
                Value::Text(
                    manifests
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            ),
            (
                "VELNOR_CACHE_BUDGET_BYTES".to_owned(),
                Value::Text(budget_bytes.to_string()),
            ),
        ],
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
    rendered
        .env
        .extend(verdicts.into_iter().flat_map(crate::public_steps::decide_env));
    if cli == InputsCli::PerRoot {
        return rendered;
    }
    rendered.condition = Some("${{ steps.inputs_save.outcome == 'success' }}".to_owned());
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
    let env = vec![
        (
            "CACHE_PREFIX".to_owned(),
            Value::Expr(format!("${{{{ needs.{lane}.outputs.{output} }}}}")),
        ),
        (
            "GITHUB_TOKEN".to_owned(),
            Value::Expr("${{ github.token }}".to_owned()),
        ),
    ];
    RenderedStep {
        name,
        env,
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
    audit_runs: Option<&str>,
) -> Vec<RenderedStep> {
    let audited = job
        .needs
        .iter()
        .any(|id| id.as_str() == "rust-inputs-audit")
        && matches!(
            step.kind,
            StepKind::CacheDecide { .. } | StepKind::CacheSave(CacheKey::Target { .. })
        );
    let extra = if audited {
        Some("${{ steps.restored.outputs.expired != 'true' || needs.rust-inputs-audit.result == 'success' }}".to_owned())
    } else if matches!(step.kind, StepKind::CacheSave(CacheKey::Sources { .. })) {
        fetched(job)
    } else {
        None
    };
    for body in &mut bodies {
        body.condition = crate::gate::all_conditions(body.condition.take(), extra.clone());
        body.condition =
            crate::gate::all_conditions(audit_runs.map(str::to_owned), body.condition.take());
    }
    bodies
}

fn fetched(job: &velnor_model::Job) -> Option<String> {
    job.steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::SourcesFetch { position, .. } => {
                Some((*position, crate::gate::sources_fetch_step(*position)))
            }
            StepKind::SourcesFetchAll { manifests, .. } => Some((0, last_fetch_all_id(manifests))),
            _ => None,
        })
        .max_by_key(|(position, _)| *position)
        .map(|(_, id)| format!("${{{{ steps.{id}.outcome == 'success' }}}}"))
}

const FETCH_ROOTS: &str = "velnor-ci sources fetch --roots";

fn fetch_chunks(manifests: &[RepoPath]) -> Vec<Vec<String>> {
    let budget = MAX_RUN_CHARACTERS
        - crate::helper_integrity::INVOCATION_PREFIX.len()
        - (FETCH_ROOTS.len() - "velnor-ci ".len());
    let mut chunks: Vec<Vec<String>> = vec![Vec::new()];
    let mut used = 0;
    for manifest in manifests {
        let word = manifest.parent().map_or_else(
            || ".".to_owned(),
            |directory| shell_word(&format!("./{directory}")),
        );
        let cost = word.chars().count() + 1;
        if used + cost > budget && used > 0 {
            chunks.push(Vec::new());
            used = 0;
        }
        used += cost;
        if let Some(last) = chunks.last_mut() {
            last.push(word);
        }
    }
    chunks
}

fn fetch_all_id(chunk: usize) -> String {
    if chunk == 0 {
        crate::gate::SOURCES_FETCH_ALL_STEP.to_owned()
    } else {
        format!("{}_{chunk}", crate::gate::SOURCES_FETCH_ALL_STEP)
    }
}

fn last_fetch_all_id(manifests: &[RepoPath]) -> String {
    fetch_all_id(fetch_chunks(manifests).len() - 1)
}

fn fetch_all_steps(manifests: &[RepoPath], cache_debug: bool) -> Vec<RenderedStep> {
    fetch_chunks(manifests)
        .into_iter()
        .enumerate()
        .map(|(chunk, roots)| RenderedStep {
            env: cache_debug
                .then(crate::cargo::cache_debug_env)
                .into_iter()
                .collect(),
            id: Some(fetch_all_id(chunk)),
            name: if chunk == 0 {
                "Fetch the sources of every root".to_owned()
            } else {
                format!("Fetch the sources of every root part {}", chunk + 1)
            },
            condition: chunk.checked_sub(1).map(|previous| {
                format!(
                    "${{{{ steps.{}.outcome == 'success' }}}}",
                    fetch_all_id(previous)
                )
            }),
            continue_on_error: true,
            run: Some(format!("{FETCH_ROOTS} {}", roots.join(" "))),
            ..RenderedStep::default()
        })
        .collect()
}

pub(crate) fn sources_fetch_all(manifests: &[RepoPath], cache_debug: bool) -> RenderedStep {
    fetch_all_steps(manifests, cache_debug)
        .into_iter()
        .next()
        .unwrap_or_default()
}

pub(crate) fn sources_fetch_all_rest(
    manifests: &[RepoPath],
    cache_debug: bool,
) -> Vec<RenderedStep> {
    fetch_all_steps(manifests, cache_debug)
        .into_iter()
        .skip(1)
        .collect()
}

pub(crate) fn sources_fetch(position: usize, lane: crate::cargo::CargoLane) -> RenderedStep {
    let mut fetch = crate::cargo::cargo_fetch(lane);
    fetch.id = Some(crate::gate::sources_fetch_step(position));
    fetch.name = format!("Fetch the sources of root {position}");
    fetch.continue_on_error = true;
    fetch
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
        has(|kind| {
            matches!(
                kind,
                StepKind::Helper(HelperCommand::InputsAudit) | StepKind::InputsAuditRoot(..)
            )
        }),
    )
}

pub(crate) fn cache_budget(bytes: u64) -> RenderedStep {
    let env = vec![(
        "GITHUB_TOKEN".to_owned(),
        Value::Expr("${{ github.token }}".to_owned()),
    )];
    RenderedStep {
        name: "Enforce the repository cache budget".to_owned(),
        env,
        run: Some(format!("velnor-ci cache budget --budget-bytes {bytes}")),
        ..RenderedStep::default()
    }
}
