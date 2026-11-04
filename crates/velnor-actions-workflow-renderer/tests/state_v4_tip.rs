// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{Job, StepKind, StepRole};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Gh, MAIN, REPO};
use crate::state_v4_seal::{seal_job, step_of};

const HEAD: &str = "1111111111111111111111111111111111111111";
const NEWER: &str = "2222222222222222222222222222222222222222";

fn run_tip(scene: &Scene, gh: &Gh, job: &Job, head: &str) -> Result<String, Box<dyn Error>> {
    let step = step_of(job, StepRole::StateSealTip)?;
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(fail("the tip step is not a shell step"));
    };
    match std::fs::remove_file(&scene.github_output) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let dir = gh.dir();
    scene.run_script(
        run.get(2).ok_or_else(|| fail("no script"))?,
        &[
            ("FAKE_GH_DIR", dir.as_str()),
            ("GITHUB_REPOSITORY", REPO),
            ("GITHUB_SHA", head),
            ("VELNOR_DEFAULT_REF", MAIN),
            ("GH_TOKEN", "fixture"),
        ],
        None,
    )
}

#[test]
#[cfg(unix)]
fn only_the_head_that_is_still_the_tip_of_main_may_be_sealed() -> Result<(), Box<dyn Error>> {
    let job = seal_job()?;
    let scene = Scene::new("seal-tip", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;

    gh.tip(HEAD)?;
    let log = run_tip(&scene, &gh, &job, HEAD)?;
    if scene.output_value("current")? != "true" || !log.contains("is the tip of") {
        return Err(fail(&format!("the tip head was refused: {log}")));
    }

    gh.tip(NEWER)?;
    let log = run_tip(&scene, &gh, &job, HEAD)?;
    if scene.output_value("current")? != "false"
        || !log.contains(&format!(
            "state seal skipped: head {HEAD} is behind refs/heads/main at {NEWER}"
        ))
    {
        return Err(fail(&format!("a head behind the tip was sealed: {log}")));
    }

    gh.mode("tip_fail")?;
    let log = run_tip(&scene, &gh, &job, HEAD)?;
    if scene.output_value("current")? != "false" || !log.contains("tip_unreadable") {
        return Err(fail(&format!(
            "an unreadable tip was treated as current: {log}"
        )));
    }

    gh.mode("ok")?;
    std::fs::write(gh.dir() + "/tip", "not-a-sha\n")?;
    let log = run_tip(&scene, &gh, &job, HEAD)?;
    if scene.output_value("current")? != "false" || !log.contains("tip_malformed") {
        return Err(fail(&format!(
            "a malformed tip was treated as current: {log}"
        )));
    }
    scene.finish()
}
