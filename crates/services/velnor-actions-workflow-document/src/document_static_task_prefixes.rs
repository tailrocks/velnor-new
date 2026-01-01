//! Exact static composites for repeated Rust task setup prefixes.

use std::collections::BTreeSet;

use velnor_actions_contract_workflow::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract_workflow::{Step, StepRole};
use velnor_actions_workflow_jobs::RenderContext;
use velnor_actions_workflow_steps::RenderError;

use crate::lane_share::LaneShare;

/// Factor only identical typed setup sequences shared by both Rust providers.
pub(crate) fn factor_static_task_prefixes(
    shared: &mut LaneShare,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let mut groups: Vec<(Vec<Step>, Vec<String>)> = Vec::new();
    for (id, steps) in &shared.preludes {
        if !id.starts_with("rust-") || !is_static_task_prefix(steps) {
            continue;
        }
        if let Some((_, owners)) = groups.iter_mut().find(|(known, _)| known == steps) {
            owners.push(id.clone());
        } else {
            groups.push((steps.clone(), vec![id.clone()]));
        }
    }
    for (steps, owners) in groups {
        if has_both_providers(&owners) && owners.len() >= 2 {
            add_static_prefix_action(&steps, &owners, shared, ctx)?;
        }
    }
    Ok(())
}

fn is_static_task_prefix(steps: &[Step]) -> bool {
    const ROLES: [StepRole; 10] = [
        StepRole::PreseedDownload,
        StepRole::PreseedVerifyManifest,
        StepRole::PreseedStage,
        StepRole::DownloadPlan,
        StepRole::PreparePinnedTools,
        StepRole::PrepareRustComponents,
        StepRole::CargoSourcesRestore,
        StepRole::MbxPreflight,
        StepRole::MbxCache,
        StepRole::MbxVersionCheck,
    ];
    steps.len() == ROLES.len()
        && steps.iter().zip(ROLES).all(|(step, role)| {
            step.role == Some(role) && step.id.is_none() && step.condition.is_none()
        })
        && steps.iter().all(|step| {
            super::step_values(step)
                .into_iter()
                .all(static_prefix_value_supported)
        })
}

fn static_prefix_value_supported(value: &str) -> bool {
    ![
        "needs", "matrix", "strategy", "steps", "inputs", "secrets", "vars", "env", "job", "jobs",
    ]
    .into_iter()
    .any(|context| {
        value.contains(&format!("{context}.")) || super::contains_indexed_context(value, context)
    }) && !value.contains(".outputs.")
}

fn has_both_providers(owners: &[String]) -> bool {
    owners.iter().any(|id| id.ends_with(HOSTED_SUFFIX))
        && owners.iter().any(|id| id.ends_with(SCALE_SUFFIX))
}

fn add_static_prefix_action(
    steps: &[Step],
    owners: &[String],
    shared: &mut LaneShare,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    for owner in owners {
        if !shared
            .preludes
            .get(owner)
            .is_some_and(|prelude| prelude == steps)
        {
            return Err(crate::lane_share_sections::task_factor_error(
                owner,
                "prelude_changed",
            ));
        }
        if !shared
            .calls
            .get(owner)
            .is_some_and(|call| call.prelude_uses.is_empty())
        {
            return Err(crate::lane_share_sections::task_factor_error(
                owner,
                "prelude_call_unexpected",
            ));
        }
    }
    let logical = next_rust_action_name(shared)?;
    let file = crate::lane_share_sections::composite_file(&logical, steps, ctx)?;
    if shared
        .files
        .iter()
        .any(|existing| existing.path == file.path)
    {
        return Err(crate::lane_share_sections::task_factor_error(
            &logical,
            "path_collision",
        ));
    }
    shared.files.push(file);
    let uses = format!("./.github/actions/{logical}");
    for owner in owners {
        let Some(call) = shared.calls.get_mut(owner) else {
            return Err(crate::lane_share_sections::task_factor_error(
                owner,
                "body_call_missing",
            ));
        };
        call.prelude_uses.push(uses.clone());
        let Some(prelude) = shared.preludes.get_mut(owner) else {
            return Err(crate::lane_share_sections::task_factor_error(
                owner,
                "prelude_missing",
            ));
        };
        prelude.clear();
    }
    Ok(())
}

fn next_rust_action_name(shared: &LaneShare) -> Result<String, RenderError> {
    let mut used = shared
        .files
        .iter()
        .filter_map(|file| {
            file.path
                .strip_prefix(".github/actions/")
                .and_then(|path| path.strip_suffix("/action.yml"))
        })
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    for call in shared.calls.values() {
        if let Some(name) = call.uses.strip_prefix("./.github/actions/") {
            used.insert(name.to_owned());
        }
        used.extend(
            call.prelude_uses
                .iter()
                .filter_map(|uses| uses.strip_prefix("./.github/actions/").map(str::to_owned)),
        );
    }
    (0..=used.len())
        .map(|index| format!("rust-{index}"))
        .find(|name| !used.contains(name))
        .ok_or_else(|| {
            crate::lane_share_sections::task_factor_error("rust-static-prefix", "path_collision")
        })
}
