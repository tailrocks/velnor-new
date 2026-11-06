// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use velnor_actions_contract::workflow::cache_family::StateUnit;

use super::super::{RunningSelector, wait_until};
use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_clock::SupervisorClock;
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
use crate::gc1_seal_units::{
    expect, expect_flag, group_is_live, process_group, process_is_running, produce_flag,
    selected_unit, unit_key,
};

#[test]
fn resistant_verifier_timeout_clears_its_unit_and_keeps_sibling() -> Result<(), Box<dyn Error>> {
    let mut case = ResistantCase::start()?;
    let verifier = read_pid(&case.dir.join("resistant-verifier"))?;
    let before = fs::read_to_string(case.scene.runner.join("velnor/state-select/u-r0/part"))?;
    if !before.lines().any(|line| line == case.earlier) {
        return Err(fail(
            "prior signed candidate was not accepted before timeout",
        ));
    }
    let child = read_pid(&case.dir.join("resistant-child"))?;
    let group = process_group(verifier).ok_or_else(|| fail("resistant verifier has no group"))?;
    if process_group(child) != Some(group)
        || !process_is_running(verifier)
        || !process_is_running(child)
    {
        return Err(fail(
            "resistant verifier and child did not start in one group",
        ));
    }
    let sibling_status = case.scene.runner.join("velnor/state-select/status-u1");
    wait_until(
        || case.dir.join("sibling-started").exists(),
        Duration::from_secs(8),
    )?;
    case.clock.block("sibling")?;
    case.clock
        .wait_for_block("sibling", Duration::from_secs(8))?;
    fs::write(case.dir.join("sibling-release"), "release\n")?;
    wait_until(|| sibling_status.exists(), Duration::from_secs(8))?;
    if !group_is_live(group) {
        return Err(fail(
            "healthy sibling was not eligible before verifier timeout",
        ));
    }
    case.clock.release_block("sibling")?;
    let early = case.selector.observe(Duration::from_secs(15))?;
    let status = if let Some(status) = early {
        status
    } else {
        case.clock.advance("resistant-cleanup", 21)?;
        case.selector.wait(Duration::from_secs(10))?
    };
    if early.is_none() {
        return Err(fail("resistant verifier remained after its call deadline"));
    }
    if !status.success() {
        return Err(fail("resistant verifier expiry failed the selector step"));
    }
    if process_is_running(verifier) || process_is_running(child) || group_is_live(group) {
        return Err(fail("resistant verifier group survived publication"));
    }
    fs::write(case.dir.join("publication-observed"), "published\n")?;
    thread::sleep(Duration::from_millis(300));
    if case.dir.join("post-publication-write").exists() {
        return Err(fail("resistant descendant wrote after publication"));
    }
    case.assert_outputs()?;
    case.scene.finish()
}

fn read_pid(path: &std::path::Path) -> Result<u32, Box<dyn Error>> {
    wait_until(|| path.exists(), Duration::from_secs(10))?;
    Ok(fs::read_to_string(path)?.trim().parse()?)
}

struct ResistantCase {
    scene: Scene,
    clock: SupervisorClock,
    selector: RunningSelector,
    dir: PathBuf,
    sibling: String,
    earlier: String,
}

impl ResistantCase {
    fn start() -> Result<Self, Box<dyn Error>> {
        let scene = Scene::new("resistant-verifier", crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let earlier = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
        let hung = unit_key(StateUnit::Union, ".", 0, 0x1000, 2)?;
        let sibling = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
        gh.list(&[
            ("2026-11-04T04:00:00Z", MAIN, &earlier),
            ("2026-11-04T03:00:00Z", MAIN, &hung),
            ("2026-11-04T02:00:00Z", MAIN, &sibling),
        ])?;
        gh.attest(&[
            Attestation::signed(&earlier),
            Attestation::signed(&hung),
            Attestation::signed(&sibling),
        ])?;
        let dir = PathBuf::from(gh.dir());
        fs::write(dir.join("resistant-keys"), format!("{hung}\n"))?;
        let clock = SupervisorClock::install(&scene, &gh)?;
        let gate = dir.join("sibling-release").to_string_lossy().into_owned();
        let started = dir.join("sibling-started").to_string_lossy().into_owned();
        let python = clock
            .real_python()
            .to_str()
            .ok_or_else(|| fail("invalid Python path"))?;
        let selector = RunningSelector::new(
            crate::gc1_seal_units::spawn_select_units(
                &scene,
                &gh,
                &[(StateUnit::Union, "."), (StateUnit::Union, "b")],
                &[
                    ("VELNOR_STATE_GH_TIMEOUT", "10"),
                    ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
                    ("VELNOR_STATE_SELECT_DEADLINE", "30"),
                    ("FAKE_REAL_PYTHON3", python),
                    ("FAKE_GH_VERIFY_GATE", &gate),
                    ("FAKE_GH_VERIFY_GATE_KEY", &sibling),
                    ("FAKE_GH_VERIFY_STARTED", &started),
                    ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
                ],
            )?,
            &gh,
        );
        Ok(Self {
            scene,
            clock,
            selector,
            dir,
            sibling,
            earlier,
        })
    }

    fn assert_outputs(&self) -> Result<(), Box<dyn Error>> {
        expect(
            &selected_unit(&self.scene, StateUnit::Union, ".")?,
            &[],
            "resistant verifier unit",
        )?;
        expect_flag(
            &produce_flag(&self.scene, StateUnit::Union, ".")?,
            "true",
            "resistant verifier unit",
        )?;
        expect(
            &selected_unit(&self.scene, StateUnit::Union, "b")?,
            std::slice::from_ref(&self.sibling),
            "healthy sibling",
        )?;
        expect_flag(
            &produce_flag(&self.scene, StateUnit::Union, "b")?,
            "false",
            "healthy sibling",
        )
    }
}
