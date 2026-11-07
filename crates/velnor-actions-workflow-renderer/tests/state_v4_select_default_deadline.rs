// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_contract::workflow::cache_family::{RootSlug, StateUnit};
use velnor_actions_contract::workflow::state_seal::{state_attested_output, state_produce_output};
use velnor_actions_contract::{JobTimeout, StepKind};
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, state_select_step};

use crate::gc1_scene::{Scene, fail};
#[cfg(target_os = "linux")]
use crate::gc1_seal_clock::SupervisorClock;
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
use crate::gc1_seal_units::{run_select_units_with_timeout, selector_env_from_step, unit_key};

#[test]
#[cfg(target_os = "linux")]
fn default_unit_deadline_keeps_valid_candidates_after_eighty_seconds() -> Result<(), Box<dyn Error>>
{
    let scene = Scene::new("seal-select-default-deadline", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let newest = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let older = unit_key(StateUnit::Union, ".", 0, 0x1001, 2)?;
    gh.list(&[
        ("2026-11-04T03:00:00Z", MAIN, &newest),
        ("2026-11-04T02:00:00Z", MAIN, &older),
    ])?;
    gh.attest(&[Attestation::signed(&newest), Attestation::signed(&older)])?;
    let clock = SupervisorClock::install(&scene, &gh)?;
    run_deadline(&scene, &gh, &clock, &[])?;
    assert_valid_deadline_outputs(&scene, &gh, &[newest, older])?;
    assert_default_deadline_markers(&scene, &gh, &clock)?;
    assert_default_deadline_binding(&gh)?;
    Ok(())
}

fn run_deadline(
    scene: &Scene,
    gh: &Gh,
    clock: &SupervisorClock,
    extra: &[(&str, &str)],
) -> Result<String, Box<dyn Error>> {
    let python = clock
        .real_python()
        .to_str()
        .ok_or_else(|| fail("invalid Python path"))?;
    let mut environment = vec![
        ("FAKE_GH_CLOCK", "1"),
        ("FAKE_GH_CLOCK_STEP", "40"),
        ("FAKE_REAL_PYTHON3", python),
    ];
    environment.extend_from_slice(extra);
    run_select_units_with_timeout(
        scene,
        gh,
        &[(StateUnit::Union, ".")],
        &environment,
        JobTimeout::new(10)?,
    )
}

fn assert_valid_deadline_outputs(
    scene: &Scene,
    gh: &Gh,
    expected: &[String],
) -> Result<(), Box<dyn Error>> {
    let directory = std::path::PathBuf::from(gh.dir());
    let acknowledgments = fs::read_dir(directory)?
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
    let root = RootSlug::from_root(".")?;
    let selected = scene.output_block(&state_attested_output("union", root.as_str()))?;
    if selected != expected
        || scene.output_value(&state_produce_output("union", root.as_str()))? != "false"
    {
        return Err(fail(
            "default deadline did not select both valid candidates",
        ));
    }
    Ok(())
}

fn assert_default_deadline_markers(
    scene: &Scene,
    gh: &Gh,
    clock: &SupervisorClock,
) -> Result<(), Box<dyn Error>> {
    let directory = std::path::PathBuf::from(gh.dir());
    fs::write(directory.join("token-missing"), "stale\n")?;
    let Err(token_error) = run_deadline(scene, gh, clock, &[("GH_TOKEN", "")]) else {
        return Err(fail(
            "the deadline fixture must reject a missing child token",
        ));
    };
    if !token_error
        .to_string()
        .contains("gh child did not receive GH_TOKEN")
    {
        return Err(fail("deadline fixture missed the child-token marker"));
    }
    fs::write(directory.join("supervisor-payload-leaked"), "stale\n")?;
    let Err(payload_error) =
        run_deadline(scene, gh, clock, &[("FAKE_GH_FORCE_PAYLOAD_MARKER", "1")])
    else {
        return Err(fail("the deadline fixture must reject a payload marker"));
    };
    if !payload_error
        .to_string()
        .contains("selector internal environment reached a gh child")
    {
        return Err(fail("deadline fixture missed the payload marker"));
    }
    Ok(())
}

fn assert_default_deadline_binding(gh: &Gh) -> Result<(), Box<dyn Error>> {
    let mut step = state_select_step(
        &[SealRoot {
            unit: StateUnit::Union,
            slug: RootSlug::from_root(".")?,
        }],
        JobTimeout::new(10)?,
    )?;
    if let StepKind::Shell { env, .. } = &mut step.kind {
        env.remove("GH_TOKEN");
    } else {
        return Err(fail("default-deadline selector stopped using a shell step"));
    }
    let Err(error) = selector_env_from_step(gh, &[(StateUnit::Union, ".")], &[], &step) else {
        return Err(fail(
            "the deadline fixture must reject a missing generated token binding",
        ));
    };
    if !error
        .to_string()
        .contains("selector step lost its generated GH_TOKEN binding")
    {
        return Err(fail("deadline fixture missed the generated token binding"));
    }
    Ok(())
}
