//! Setup and exact tool-cache scaffold insertion, split under the size gate.

use velnor_actions_contract::{Job, Step, StepKind};

use crate::{
    MiseSetup, RenderError, cache_p08, cache_p08_detect::detector_words, setup::MISE_ACTION_NAME,
};

const IMAGE_STEP: &str = crate::cache_steps::TOOLS_IMAGE_IDENTITY_NAME;
const RESTORE_STEP: &str = crate::cache_steps::TOOLS_RESTORE_NAME;
const BOOTSTRAP_GUARD_STEP: &str = "Verify restored Mise bootstrap";

/// Build Setup Mise with the bootstrap isolated from installed tools.
///
/// Setup Mise may execute its existing bootstrap binary while probing the
/// version. Its separate home is therefore verified and cached as one file;
/// the tool installs remain under `MISE_DATA_DIR` used by typed later steps.
/// # Errors
pub fn mise_setup_step_p08(setup: &MiseSetup) -> Result<Step, RenderError> {
    crate::setup::mise_setup_step(setup)
}

/// Ensure image gate, exact restore, bootstrap check, and setup are ordered.
///
/// The entire scaffold is rebuilt idempotently when already present. Any
/// partial or malformed scaffold fails closed. A cache miss or unknown image
/// only skips restore/save; setup and tool preparation still run cold.
/// # Errors
pub fn ensure_setup_p08(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    target: &str,
) -> Result<(), RenderError> {
    setup.validate()?;
    let setup_indices = named_setup_indices(job);
    if setup_indices.len() > 1 {
        return invalid(job_id, "duplicate_setup_mise");
    }
    let use_setup = !setup_indices.is_empty() || always || job_uses_mise(job);
    let existing_scaffold = scaffold_indices(job_id, job)?;
    if !use_setup {
        if existing_scaffold.iter().any(Option::is_some) {
            return invalid(job_id, "orphan_tools_cache_scaffold");
        }
        return Ok(());
    }

    let specs = cache_p08::infer_job_tools(job);
    let specs = if specs.is_empty() && always {
        vec!["mise@bootstrap".to_owned()]
    } else if specs.is_empty() {
        return invalid(job_id, "setup_mise_without_tool_specs");
    } else {
        specs
    };
    let key = cache_p08::tools_cache_key_for_tools(target, &setup.version, &setup.sha256, &specs)?;
    let expected = expected_scaffold(setup, &key)?;
    validate_existing(
        job_id,
        job,
        &setup_indices,
        &existing_scaffold,
        &expected,
        setup,
    )?;

    let mut remove = existing_scaffold
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    remove.extend(setup_indices.iter().copied());
    remove.sort_unstable();
    let insertion = remove.first().copied().unwrap_or_else(|| insert_at(job));
    for index in remove.into_iter().rev() {
        job.steps.remove(index);
    }
    let insertion = insertion.min(job.steps.len());
    for (offset, step) in expected.into_iter().enumerate() {
        job.steps.insert(insertion + offset, step);
    }
    check_order(job_id, job)?;
    Ok(())
}

/// Build the four steps that define the shared tool-cache lifecycle.
fn expected_scaffold(setup: &MiseSetup, key: &str) -> Result<Vec<Step>, RenderError> {
    let guard = crate::owned_script::owned_bash_script_step(
        BOOTSTRAP_GUARD_STEP,
        &crate::mise_bootstrap_guard::mise_bootstrap_guard_script(&setup.sha256),
        std::collections::BTreeMap::new(),
    )?;
    Ok(vec![
        crate::cache_steps::tools_cache_image_identity_step()?,
        crate::cache_steps::tools_restore_step(key)?,
        guard,
        mise_setup_step_p08(setup)?,
    ])
}

/// Validate exact preexisting steps before idempotent replacement.
fn validate_existing(
    job_id: &str,
    job: &Job,
    setup_indices: &[usize],
    scaffold_indices: &[Option<usize>; 3],
    expected: &[Step],
    setup: &MiseSetup,
) -> Result<(), RenderError> {
    let scaffold_count = scaffold_indices.iter().flatten().count();
    if scaffold_count != 0 && scaffold_count != 3 {
        return invalid(job_id, "partial_tools_cache_scaffold");
    }
    if scaffold_count == 3 {
        let lifecycle = scaffold_indices
            .iter()
            .flatten()
            .copied()
            .chain(setup_indices.iter().copied())
            .collect::<Vec<_>>();
        if lifecycle.windows(2).any(|pair| pair[0] >= pair[1]) {
            return invalid(job_id, "tools_cache_lifecycle_misordered");
        }
        for (slot, expected_step) in scaffold_indices.iter().zip(expected.iter()) {
            let Some(index) = slot else {
                return invalid(job_id, "partial_tools_cache_scaffold");
            };
            if job.steps.get(*index) != Some(expected_step) {
                return invalid(job_id, "malformed_tools_cache_scaffold");
            }
        }
    }
    if let Some(index) = setup_indices.first() {
        let setup_after_mise = job.steps[..*index].iter().any(|step| {
            matches!(&step.kind, StepKind::Shell { run, .. } if detector_words(run).iter().any(|word| word == "mise"))
        });
        if setup_after_mise {
            return invalid(job_id, "setup_mise_misordered");
        }
        let current = job.steps.get(*index);
        let expected_setup = expected.get(3);
        let legacy_setup = crate::setup::mise_setup_step(setup).ok();
        if current != expected_setup && current != legacy_setup.as_ref() {
            return invalid(job_id, "setup_mise_malformed");
        }
    }
    if scaffold_count == 3 && setup_indices.is_empty() {
        return invalid(job_id, "incomplete_tools_cache_lifecycle");
    }
    Ok(())
}

/// Positions of the three cache steps, in fixed order.
fn scaffold_indices(job_id: &str, job: &Job) -> Result<[Option<usize>; 3], RenderError> {
    let names = [IMAGE_STEP, RESTORE_STEP, BOOTSTRAP_GUARD_STEP];
    let mut positions = [None; 3];
    for (slot, name) in names.into_iter().enumerate() {
        let mut matches = job
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| step.name == name)
            .map(|(index, _)| index);
        positions[slot] = matches.next();
        if matches.next().is_some() {
            return invalid(job_id, "duplicate_tools_cache_scaffold");
        }
    }
    Ok(positions)
}

/// Setup actions matching the pinned Mise action identity.
fn named_setup_indices(job: &Job) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with(&format!("{MISE_ACTION_NAME}@")))
        })
        .map(|(index, _)| index)
        .collect()
}

/// True when any shell step invokes `mise`.
fn job_uses_mise(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Shell { run, .. } if detector_words(run).iter().any(|word| word == "mise"))
    })
}

/// Ensure the lifecycle precedes every Mise use.
fn check_order(job_id: &str, job: &Job) -> Result<(), RenderError> {
    let names = [
        IMAGE_STEP,
        RESTORE_STEP,
        BOOTSTRAP_GUARD_STEP,
        crate::setup::SETUP_MISE_NAME,
    ];
    let positions = names
        .iter()
        .map(|name| job.steps.iter().position(|step| step.name == *name))
        .collect::<Vec<_>>();
    if positions.iter().any(Option::is_none) || positions.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return invalid(job_id, "tools_cache_lifecycle_misordered");
    }
    if let Some(first_mise) = job.steps.iter().position(|step| {
        matches!(&step.kind, StepKind::Shell { run, .. } if detector_words(run).iter().any(|word| word == "mise"))
    }) && positions[3].is_some_and(|setup| setup > first_mise)
    {
        return invalid(job_id, "setup_mise_misordered");
    }
    Ok(())
}

/// Find the insertion point after Checkout, else at the job front.
fn insert_at(job: &Job) -> usize {
    job.steps
        .first()
        .filter(|step| step.name == "Checkout")
        .map_or(0, |_| 1)
}

/// Return a stable structural error.
fn invalid<T>(job_id: &str, reason: &str) -> Result<T, RenderError> {
    Err(RenderError::InvalidWorkflow(format!("{reason}:{job_id}")))
}
