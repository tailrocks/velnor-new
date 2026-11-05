// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::workflow::cache_family::{RootSlug, StateUnit};
use velnor_actions_contract::workflow::state_seal::{state_attested_output, state_produce_output};
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, state_select_step};

use crate::gc1_scene::{Scene, fail};
#[cfg(target_os = "linux")]
use crate::gc1_seal_clock::SupervisorClock;
use crate::gc1_seal_support::{Attestation, Gh, MAIN, REPO, script_of};
use crate::gc1_seal_units::{unit_compat, unit_key, unit_snapshot};

#[test]
#[cfg(target_os = "linux")]
fn default_unit_deadline_keeps_valid_candidates_after_eighty_seconds() -> Result<(), Box<dyn Error>>
{
    let scene = Scene::new("seal-select-default-deadline", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let root = ".";
    let newest = unit_key(StateUnit::Union, root, 0, 0x1000, 1)?;
    let older = unit_key(StateUnit::Union, root, 0, 0x1001, 2)?;
    gh.list(&[
        ("2026-11-04T03:00:00Z", MAIN, &newest),
        ("2026-11-04T02:00:00Z", MAIN, &older),
    ])?;
    gh.attest(&[Attestation::signed(&newest), Attestation::signed(&older)])?;

    let clock = SupervisorClock::install(&scene, &gh)?;

    let root_slug = RootSlug::from_root(root)?;
    let roots = [SealRoot {
        unit: StateUnit::Union,
        slug: root_slug.clone(),
    }];
    let script = script_of(&state_select_step(&roots, JobTimeout::new(10)?)?)?;
    let dir = gh.dir();
    let compat = unit_compat(0);
    let snapshot = unit_snapshot(0);
    let env = [
        ("FAKE_GH_DIR", dir.as_str()),
        ("FAKE_GH_CLOCK", "1"),
        ("FAKE_GH_CLOCK_STEP", "40"),
        (
            "FAKE_REAL_PYTHON3",
            clock
                .real_python()
                .to_str()
                .ok_or_else(|| fail("invalid Python path"))?,
        ),
        ("GITHUB_REPOSITORY", REPO),
        (
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
        ),
        ("GH_TOKEN", "fixture"),
        ("VELNOR_STATE_COMPAT_R0", compat.as_str()),
        ("VELNOR_STATE_SNAPSHOT_R0", snapshot.as_str()),
    ];
    scene.run_script(&script, &env, None)?;

    let acknowledgments = fs::read_dir(gh.dir())?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("clock-ack-")
        })
        .count();
    if acknowledgments != 2 {
        return Err(fail(&format!(
            "synthetic clock advanced {acknowledgments} times, wanted two"
        )));
    }
    let selected = scene.output_block(&state_attested_output("union", root_slug.as_str()))?;
    if selected != [newest, older] {
        return Err(fail(&format!("default deadline selected {selected:?}")));
    }
    let produced = scene.output_value(&state_produce_output("union", root_slug.as_str()))?;
    if produced != "false" {
        return Err(fail(&format!(
            "valid attestations selected cold: {produced}"
        )));
    }
    Ok(())
}
