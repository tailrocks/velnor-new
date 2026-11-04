// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::RootSlug;
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_PENDING_BYTES_ENV, STATE_SEAL_PENDING_ENV, state_seal_job_id,
};
use velnor_actions_contract::{StepKind, StepRole};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Gh, REPO};
use crate::state_v4_seal::seal_jobs;

const BUDGET: u64 = 8_000_000_000;
const CEILING_FILE: u64 = 1_610_612_736;
const CEILING_BOUND: u64 = 1_616_920_640;
const SMALL_FILE: u64 = 1_000;
const SMALL_BOUND: u64 = 10_339;

struct Admit {
    script: String,
}

impl Admit {
    fn new() -> Result<Self, Box<dyn Error>> {
        let jobs = seal_jobs(&["."])?;
        let job = jobs
            .get(&state_seal_job_id(&RootSlug::from_root(".")?))
            .ok_or_else(|| fail("seal job missing"))?;
        let step = job
            .steps
            .iter()
            .find(|step| step.role == Some(StepRole::StateSealAdmit))
            .ok_or_else(|| fail("admit step missing"))?;
        let StepKind::Shell { run, .. } = &step.kind else {
            return Err(fail("admit step is not a shell step"));
        };
        Ok(Self {
            script: run.get(2).cloned().ok_or_else(|| fail("no script"))?,
        })
    }

    fn run(
        &self,
        scene: &Scene,
        gh: &Gh,
        size: Option<u64>,
        pending: &str,
        pending_bytes: u64,
    ) -> Result<(bool, String), Box<dyn Error>> {
        let payload = scene.payload();
        if std::fs::symlink_metadata(&payload).is_ok() {
            std::fs::remove_file(&payload)?;
        }
        std::fs::create_dir_all(payload.parent().ok_or_else(|| fail("no parent"))?)?;
        if let Some(size) = size {
            std::fs::File::create(&payload)?.set_len(size)?;
        }
        match std::fs::remove_file(&scene.github_output) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.into());
            }
            _ => {}
        }
        let dir = gh.dir();
        let bytes = pending_bytes.to_string();
        let output = scene.run_script(
            &self.script,
            &[
                ("FAKE_GH_DIR", dir.as_str()),
                ("GITHUB_REPOSITORY", REPO),
                ("GH_TOKEN", "fixture"),
                (STATE_SEAL_PENDING_ENV, pending),
                (STATE_SEAL_PENDING_BYTES_ENV, bytes.as_str()),
            ],
            None,
        )?;
        let admitted = scene.output_value("admitted")? == "true";
        Ok((admitted, output))
    }
}

fn expect(
    got: &(bool, String),
    admitted: bool,
    label: &str,
    evidence: &[&str],
) -> Result<(), Box<dyn Error>> {
    if got.0 != admitted {
        return Err(fail(&format!(
            "{label}: admitted={}, wanted {admitted}: {}",
            got.0, got.1
        )));
    }
    for needle in evidence {
        if !got.1.contains(needle) {
            return Err(fail(&format!("{label}: output lost {needle}: {}", got.1)));
        }
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn state_save_obeys_the_8e9_total() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("admit", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let admit = Admit::new()?;
    let ceiling = Some(CEILING_FILE);

    gh.usage(BUDGET - CEILING_BOUND)?;
    expect(
        &admit.run(&scene, &gh, ceiling, "", 0)?,
        true,
        "exactly the budget",
        &[
            "state save admitted",
            "bound=1616920640",
            "budget=8000000000",
        ],
    )?;
    gh.usage(BUDGET - CEILING_BOUND + 1)?;
    expect(
        &admit.run(&scene, &gh, ceiling, "", 0)?,
        false,
        "one byte over",
        &[
            "over_budget usage=6383079361 bound=1616920640 reserved=0 budget=8000000000",
            "::warning::cache_unavailable",
        ],
    )?;

    gh.usage(5_112_868_912)?;
    expect(
        &admit.run(&scene, &gh, ceiling, "", 0)?,
        true,
        "chain case: usage plus one new entry",
        &["usage=5112868912"],
    )?;

    gh.usage(BUDGET - 2 * CEILING_BOUND)?;
    expect(
        &admit.run(&scene, &gh, ceiling, "true", CEILING_BOUND)?,
        true,
        "a later eligible root fits exactly",
        &["reserved=1616920640"],
    )?;
    gh.usage(BUDGET - 2 * CEILING_BOUND + 1)?;
    expect(
        &admit.run(&scene, &gh, ceiling, "true", CEILING_BOUND)?,
        false,
        "a later eligible root does not fit",
        &["reserved=1616920640"],
    )?;
    expect(
        &admit.run(&scene, &gh, ceiling, "false", CEILING_BOUND)?,
        true,
        "a later root that is not eligible reserves nothing",
        &["reserved=0"],
    )?;

    scene.finish()
}

#[test]
#[cfg(unix)]
fn state_save_reserves_the_outer_bound_and_fails_closed() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("admit-outer", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let admit = Admit::new()?;
    let ceiling = Some(CEILING_FILE);
    gh.usage(BUDGET - SMALL_BOUND)?;
    expect(
        &admit.run(&scene, &gh, Some(SMALL_FILE), "", 0)?,
        true,
        "outer bound of a small file",
        &["bound=10339"],
    )?;
    gh.usage(BUDGET - SMALL_BOUND + 1)?;
    expect(
        &admit.run(&scene, &gh, Some(SMALL_FILE), "", 0)?,
        false,
        "the inner size is not the reservation",
        &["bound=10339"],
    )?;

    gh.usage(0)?;
    gh.mode("usage_fail")?;
    expect(
        &admit.run(&scene, &gh, ceiling, "", 0)?,
        false,
        "usage API failure refuses the save",
        &["usage_unavailable"],
    )?;
    gh.mode("ok")?;
    expect(
        &admit.run(&scene, &gh, None, "", 0)?,
        false,
        "no payload",
        &["payload_unreadable"],
    )?;
    scene.finish()
}
