// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

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
    match observe_cleanup_or_completion(&mut case, Duration::from_secs(15))? {
        CleanupObservation::GateHeld => {
            let published_while_alive = validate_cleanup_gate(&case, group)?;
            case.cleanup_gate.release()?;
            let status = case.selector.wait(Duration::from_secs(10))?;
            require_success(status, "resistant verifier expiry failed the selector step")?;
            assert_resistant_processes_reaped(verifier, child, group)?;
            finish_publication_observation(&case)?;
            if published_while_alive {
                return Err(fail(
                    "selector published outputs while resistant group was alive",
                ));
            }
            case.assert_outputs()?;
            case.scene.finish()
        }
        CleanupObservation::Completed(status) => {
            case.cleanup_gate.release()?;
            require_success(status, "resistant verifier expiry failed the selector step")?;
            assert_resistant_processes_reaped(verifier, child, group)?;
            Err(fail(
                "resistant verifier completed without observed cleanup gate",
            ))
        }
        CleanupObservation::Deadline => {
            let verifier_was_live = process_is_running(verifier);
            let child_was_live = process_is_running(child);
            eprintln!(
                "X101 resistant observation=deadline verifier_live={verifier_was_live} child_live={child_was_live}"
            );
            case.cleanup_gate.release()?;
            case.clock.advance("resistant-cleanup", 21)?;
            let status = case.selector.wait(Duration::from_secs(10))?;
            require_success(status, "resistant verifier expiry failed the selector step")?;
            assert_resistant_processes_reaped(verifier, child, group)?;
            if !verifier_was_live && !child_was_live {
                return Err(fail(
                    "resistant verifier processes were not live at the observation deadline",
                ));
            }
            Err(fail("resistant verifier remained after its call deadline"))
        }
    }
}

enum CleanupObservation {
    GateHeld,
    Completed(std::process::ExitStatus),
    Deadline,
}

fn observe_cleanup_or_completion(
    case: &mut ResistantCase,
    duration: Duration,
) -> Result<CleanupObservation, Box<dyn Error>> {
    let deadline = Instant::now() + duration;
    loop {
        if cleanup_gate_blocked(case) {
            return Ok(CleanupObservation::GateHeld);
        }
        if let Some(status) = case.selector.observe(Duration::ZERO)? {
            if cleanup_gate_blocked(case) {
                return Ok(CleanupObservation::GateHeld);
            }
            return Ok(CleanupObservation::Completed(status));
        }
        if Instant::now() >= deadline {
            if cleanup_gate_blocked(case) {
                return Ok(CleanupObservation::GateHeld);
            }
            let status = case.selector.observe(Duration::ZERO)?;
            if cleanup_gate_blocked(case) {
                return Ok(CleanupObservation::GateHeld);
            }
            if let Some(status) = status {
                return Ok(CleanupObservation::Completed(status));
            }
            return Ok(CleanupObservation::Deadline);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn cleanup_gate_blocked(case: &ResistantCase) -> bool {
    PathBuf::from(format!("{}.blocked", case.cleanup_gate.0.display())).exists()
}

fn validate_cleanup_gate(case: &ResistantCase, group: u32) -> Result<bool, Box<dyn Error>> {
    if !case
        .scene
        .runner
        .join("velnor/state-select/status-u0")
        .exists()
    {
        return Err(fail("cleanup gate has no u0 guardian status"));
    }
    let logged_group = last_logged_group(&case.group_log)?;
    if logged_group != group
        || process_group(group) != Some(group)
        || !PathBuf::from(format!("/proc/{group}")).exists()
        || !group_is_live(group)
    {
        return Err(fail("cleanup gate did not hold the live u0 guardian"));
    }
    let published_while_alive = match fs::read(&case.scene.github_output) {
        Ok(bytes) => !bytes.is_empty(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    Ok(published_while_alive)
}

fn require_success(status: std::process::ExitStatus, message: &str) -> Result<(), Box<dyn Error>> {
    if status.success() {
        Ok(())
    } else {
        Err(fail(message))
    }
}

fn assert_resistant_processes_reaped(
    verifier: u32,
    child: u32,
    group: u32,
) -> Result<(), Box<dyn Error>> {
    if process_is_running(verifier)
        || process_is_running(child)
        || process_is_running(group)
        || PathBuf::from(format!("/proc/{group}")).exists()
        || group_is_live(group)
    {
        return Err(fail("resistant verifier group survived publication"));
    }
    Ok(())
}

fn finish_publication_observation(case: &ResistantCase) -> Result<(), Box<dyn Error>> {
    fs::write(case.dir.join("publication-observed"), "published\n")?;
    thread::sleep(Duration::from_millis(300));
    if case.dir.join("post-publication-write").exists() {
        return Err(fail("resistant descendant wrote after publication"));
    }
    Ok(())
}

fn last_logged_group(path: &std::path::Path) -> Result<u32, Box<dyn Error>> {
    fs::read_to_string(path)?
        .lines()
        .last()
        .ok_or_else(|| fail("cleanup gate did not log a SIGKILL group"))?
        .parse()
        .map_err(Into::into)
}

fn read_pid(path: &std::path::Path) -> Result<u32, Box<dyn Error>> {
    wait_until(|| path.exists(), Duration::from_secs(10))?;
    Ok(fs::read_to_string(path)?.trim().parse()?)
}

struct ResistantCase {
    cleanup_gate: CleanupGate,
    selector: RunningSelector,
    scene: Scene,
    clock: SupervisorClock,
    group_log: PathBuf,
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
        let cleanup_gate = dir.join("cleanup-release");
        let cleanup_gate_value = cleanup_gate.to_string_lossy().into_owned();
        let group_log = dir.join("group-log");
        let group_log_value = group_log.to_string_lossy().into_owned();
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
                    ("FAKE_SUPERVISOR_CLEANUP_GATE", &cleanup_gate_value),
                    ("FAKE_SUPERVISOR_GROUP_LOG", &group_log_value),
                ],
            )?,
            &gh,
        );
        let cleanup_guard = CleanupGate(cleanup_gate.clone());
        Ok(Self {
            cleanup_gate: cleanup_guard,
            selector,
            scene,
            clock,
            group_log,
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

struct CleanupGate(PathBuf);

impl CleanupGate {
    fn release(&self) -> Result<(), Box<dyn Error>> {
        fs::write(&self.0, "release\n")?;
        Ok(())
    }
}

impl Drop for CleanupGate {
    fn drop(&mut self) {
        drop(fs::write(&self.0, "release\n"));
    }
}
