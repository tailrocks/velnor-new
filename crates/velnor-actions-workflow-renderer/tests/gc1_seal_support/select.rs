// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::{RootSlug, StateUnit};
use velnor_actions_contract::workflow::state_seal::state_attested_output;
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealUnit, state_select_step};

use super::{Gh, OTHER_WORKFLOW_REF, REPO, script_of, slice_of};
use crate::gc1_scene::Scene;

fn seal_unit(root: &str) -> Result<SealUnit, Box<dyn Error>> {
    Ok(SealUnit {
        slice: slice_of(root)?,
        root: RootSlug::from_root(root)?,
    })
}

pub(crate) fn run_select(
    scene: &Scene,
    gh: &Gh,
    root: &str,
    extra: &[(&str, &str)],
) -> Result<String, Box<dyn Error>> {
    run_select_units(scene, gh, &[seal_unit(root)?], extra)
}

pub(crate) fn run_select_units(
    scene: &Scene,
    gh: &Gh,
    units: &[SealUnit],
    extra: &[(&str, &str)],
) -> Result<String, Box<dyn Error>> {
    match std::fs::remove_file(&scene.github_output) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let dir = gh.dir();
    let mut env = vec![
        ("FAKE_GH_DIR", dir.as_str()),
        ("GITHUB_REPOSITORY", REPO),
        ("GITHUB_WORKFLOW_REF", OTHER_WORKFLOW_REF),
        ("GH_TOKEN", "fixture"),
    ];
    env.extend_from_slice(extra);
    scene.run_script(&script_of(&state_select_step(units)?)?, &env, None)
}

pub(crate) fn selected(scene: &Scene, root: &str) -> Result<Vec<String>, Box<dyn Error>> {
    selected_unit(scene, &StateUnit::Slice(slice_of(root)?))
}

pub(crate) fn selected_unit(
    scene: &Scene,
    unit: &StateUnit,
) -> Result<Vec<String>, Box<dyn Error>> {
    scene.output_block(&state_attested_output(unit))
}
