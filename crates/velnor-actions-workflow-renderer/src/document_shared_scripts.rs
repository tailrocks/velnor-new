//! Share repeated MBX verifier bodies from their exact typed step factory.
//!
//! The complete preflight/action/version-check triplet is regenerated and
//! compared before a verifier body can be moved to a marked script file.
//! Each shell step keeps its original process, credential prelude, argv shape,
//! environment, position, and identity.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, Step, StepKind, digest_b3};

use crate::lane_share::LaneShare;
use crate::render::RenderContext;
use crate::tree::RenderedFile;
use crate::{RenderError, marker, steps, toolchain_env};

#[cfg(test)]
#[path = "document_shared_scripts_tests.rs"]
mod tests;

const SCRIPT_ROOT: &str = ".github/scripts/velnor-shared";
const PREFLIGHT_BODY_DIGEST: &str =
    "b3-d27cbaf0856ff4026fbd752c2b537ca733f78bdfb252a6994c33d20f2207dd11";
const VERSION_CHECK_BODY_DIGEST: &str =
    "b3-566aabadb34068ae41f3799f5f591ed7b8c6f47057ba095f81c1039720d9ab5e";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ShellDialect {
    Sh,
    Bash,
}

impl ShellDialect {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sh => "sh",
            Self::Bash => "bash",
        }
    }

    fn interpreter(self) -> &'static str {
        self.as_str()
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Sh => "sh",
            Self::Bash => "bash",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FactoryScript {
    Preflight,
    VersionCheck,
}

impl FactoryScript {
    fn body_digest(self) -> &'static str {
        match self {
            Self::Preflight => PREFLIGHT_BODY_DIGEST,
            Self::VersionCheck => VERSION_CHECK_BODY_DIGEST,
        }
    }

    fn triplet_offset(self) -> usize {
        match self {
            Self::Preflight => 0,
            Self::VersionCheck => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ScriptKey {
    dialect: ShellDialect,
    body: String,
}

#[derive(Debug, Clone)]
struct Occurrence {
    job_id: String,
    step_index: usize,
}

/// Rewrite repeated, audited verifier bodies in place and return their files.
///
/// Only the exact three-step MBX factory output is eligible. A changed factory
/// body remains inline until its source and digest are reviewed together.
///
/// # Errors
///
/// Returns [`RenderError`] if an already-authenticated step changes before it
/// is rewritten or a generated body cannot be marked.
pub(crate) fn share_trusted_scripts(
    shared: &mut LaneShare,
    ctx: &RenderContext,
) -> Result<Vec<RenderedFile>, RenderError> {
    let checkout = steps::checkout_step(&ctx.checkout_uses)?;
    let mut groups = BTreeMap::<ScriptKey, Vec<Occurrence>>::new();
    for (job_id, job) in &shared.jobs {
        if shared.calls.contains_key(job_id) || !has_root_checkout(job, &checkout) {
            continue;
        }
        for (step_index, step) in job.steps.iter().enumerate() {
            if let Some(key) = approved_factory_script(job, step_index, step) {
                groups.entry(key).or_default().push(Occurrence {
                    job_id: job_id.clone(),
                    step_index,
                });
            }
        }
    }

    let mut files = Vec::new();
    for (key, occurrences) in groups {
        let jobs: BTreeSet<&str> = occurrences
            .iter()
            .map(|occurrence| occurrence.job_id.as_str())
            .collect();
        if jobs.len() < 2 {
            continue;
        }
        let file = script_file(&key, &ctx.generator_version)?;
        for occurrence in occurrences {
            replace_with_source(shared, &occurrence, &key, &file.path)?;
        }
        files.push(file);
    }
    Ok(files)
}

pub(crate) fn compact_oversized_workflow<F>(
    shared: &mut LaneShare,
    ctx: &RenderContext,
    mut text: String,
    render: F,
) -> Result<String, RenderError>
where
    F: Fn(&LaneShare) -> Result<String, RenderError>,
{
    if text.len() <= crate::workflow_size::MAX_WORKFLOW_BYTES {
        return Ok(text);
    }
    let files = share_trusted_scripts(shared, ctx)?;
    let changed = !files.is_empty();
    for file in files {
        push_unique_file(shared, file)?;
    }
    if changed {
        text = render(shared)?;
    }
    Ok(text)
}

fn push_unique_file(shared: &mut LaneShare, file: RenderedFile) -> Result<(), RenderError> {
    if shared
        .files
        .iter()
        .any(|existing| existing.path == file.path)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "generated_shared_file_path_collision:{}",
            file.path
        )));
    }
    shared.files.push(file);
    Ok(())
}

fn has_root_checkout(job: &Job, expected: &Step) -> bool {
    job.steps.first() == Some(expected)
        && !job.steps.iter().skip(1).any(|step| {
            step.name == "Checkout"
                || matches!(
                    &step.kind,
                    StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")
                )
        })
}

fn approved_factory_script(job: &Job, step_index: usize, step: &Step) -> Option<ScriptKey> {
    let role = if step.name == steps::MBX_PREFLIGHT_NAME {
        FactoryScript::Preflight
    } else if step.name == steps::MBX_VERSION_CHECK_NAME {
        FactoryScript::VersionCheck
    } else {
        return None;
    };
    if count_named(job, steps::MBX_PREFLIGHT_NAME) != 1
        || count_named(job, steps::MBX_VERSION_CHECK_NAME) != 1
        || count_mbx_actions(job) != 1
    {
        return None;
    }
    let start = step_index.checked_sub(role.triplet_offset())?;
    let triplet = job.steps.get(start..start + 3)?;
    let StepKind::Action { uses, with, .. } = &triplet[1].kind else {
        return None;
    };
    let version = with.get("version")?;
    let toolchain = with.get("toolchain")?;
    let StepKind::Shell { env, .. } = &triplet[0].kind else {
        return None;
    };
    let expected = steps::mbx_steps_for_driver(
        uses,
        steps::CompileDriver::Mbx,
        version,
        toolchain,
        factory_input_env(env),
    )
    .ok()??;
    if triplet != expected.as_slice() || expected[role.triplet_offset()] != *step {
        return None;
    }
    let (dialect, body) = trusted_factory_shell(step)?;
    let digest_input = format!("{}\0{}", dialect.as_str(), body);
    (digest_b3(digest_input.as_bytes()) == role.body_digest())
        .then_some(ScriptKey { dialect, body })
}

fn factory_input_env(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let scrub = toolchain_env::credential_scrub();
    env.iter()
        .filter(|(key, _)| !scrub.contains_key(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn count_named(job: &Job, name: &str) -> usize {
    job.steps.iter().filter(|step| step.name == name).count()
}

fn count_mbx_actions(job: &Job) -> usize {
    job.steps
        .iter()
        .filter(|step| {
            matches!(
                &step.kind,
                StepKind::Action { uses, .. }
                    if uses.starts_with(&format!("{}@", steps::MBX_ACTION_NAME))
            )
        })
        .count()
}

fn trusted_factory_shell(step: &Step) -> Option<(ShellDialect, String)> {
    if step.condition.is_some() {
        return None;
    }
    let StepKind::Shell { run, .. } = &step.kind else {
        return None;
    };
    let [interpreter, flag, command] = run.as_slice() else {
        return None;
    };
    if flag != "-c" {
        return None;
    }
    let dialect = match interpreter.as_str() {
        "sh" => ShellDialect::Sh,
        "bash" => ShellDialect::Bash,
        _ => return None,
    };
    let prelude = format!("{} ", toolchain_env::credential_unset_prelude());
    let body = command.strip_prefix(&prelude)?;
    if body.is_empty()
        || body.contains("${{")
        || body.contains("$BASH_SOURCE")
        || body.contains("${BASH_SOURCE")
        || body.contains("$0")
        || body.contains("${0}")
        || body.contains("LINENO")
    {
        return None;
    }
    Some((dialect, body.to_owned()))
}

fn script_file(key: &ScriptKey, version: &str) -> Result<RenderedFile, RenderError> {
    let digest_input = format!("{}\0{}", key.dialect.as_str(), key.body);
    let digest = digest_b3(digest_input.as_bytes());
    let path = format!(
        "{SCRIPT_ROOT}/{}-{digest}.{}",
        key.dialect.as_str(),
        key.dialect.extension()
    );
    let bytes = marker::with_marker(version, &key.body)?;
    Ok(RenderedFile { path, bytes })
}

fn replace_with_source(
    shared: &mut LaneShare,
    occurrence: &Occurrence,
    key: &ScriptKey,
    path: &str,
) -> Result<(), RenderError> {
    let Some(job) = shared.jobs.get_mut(&occurrence.job_id) else {
        return Err(RenderError::InvalidWorkflow(
            "shared_script_job_missing".to_owned(),
        ));
    };
    let Some(step) = job.steps.get_mut(occurrence.step_index) else {
        return Err(RenderError::InvalidWorkflow(
            "shared_script_step_missing".to_owned(),
        ));
    };
    let StepKind::Shell { run, .. } = &mut step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "shared_script_step_not_shell".to_owned(),
        ));
    };
    let expected_command = toolchain_env::with_credential_unset_script(&key.body);
    if run.len() != 3
        || run[0] != key.dialect.interpreter()
        || run[1] != "-c"
        || run[2] != expected_command
    {
        return Err(RenderError::InvalidWorkflow(
            "shared_script_argv_changed".to_owned(),
        ));
    }
    run[2] = toolchain_env::with_credential_unset_script(&format!(". './{path}'"));
    Ok(())
}
