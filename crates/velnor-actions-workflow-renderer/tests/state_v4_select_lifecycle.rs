// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use velnor_actions_contract::workflow::cache_family::StateUnit;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_SELECT_MAX_PARALLELISM;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
use crate::gc1_seal_units::{
    SupervisorClock, at, expect, expect_flag, group_is_live, hang_keys, hung_processes_gone,
    process_group, process_is_reaped, process_is_running, produce_flag, rendezvous, rows_of,
    selected_unit, spawn_select_units, unit_key,
};

const T1: &str = "2026-11-04T01:00:00Z";
const T3: &str = "2026-11-04T03:00:00Z";

#[cfg(target_os = "linux")]
struct RunningSelector {
    child: Child,
    finished: bool,
}

#[cfg(target_os = "linux")]
impl RunningSelector {
    fn new(child: Child) -> Self {
        Self {
            child,
            finished: false,
        }
    }

    fn wait(&mut self, limit: Duration) -> Result<ExitStatus, Box<dyn Error>> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.finished = true;
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(fail("selector exceeded the real-time watchdog"));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for RunningSelector {
    fn drop(&mut self) {
        if !self.finished {
            let group = format!("-{}", self.child.id());
            drop(
                Command::new("kill")
                    .args(["-TERM", "--"])
                    .arg(&group)
                    .status(),
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while group_is_live(self.child.id()) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if group_is_live(self.child.id()) {
                drop(
                    Command::new("kill")
                        .args(["-KILL", "--"])
                        .arg(group)
                        .status(),
                );
            }
            drop(self.child.wait());
        }
    }
}

#[cfg(target_os = "linux")]
fn wait_until(mut ready: impl FnMut() -> bool, limit: Duration) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if ready() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(fail(
        "fixture did not reach its barrier before the watchdog",
    ))
}

#[cfg(target_os = "linux")]
fn wait_for_rendezvous(gh: &Gh, expected: usize) -> Result<(), Box<dyn Error>> {
    let markers = PathBuf::from(gh.dir()).join("rv");
    wait_until(
        || std::fs::read_dir(&markers).is_ok_and(|entries| entries.count() >= expected),
        Duration::from_secs(20),
    )
}

#[cfg(target_os = "linux")]
fn wait_for_healthy_units_clean(
    scene: &Scene,
    gh: &Gh,
    keys: &[String],
) -> Result<(), Box<dyn Error>> {
    let verified = PathBuf::from(gh.dir()).join("verified");
    let state_dir = scene.runner.join("velnor/state-select");
    wait_until(
        || {
            let Ok(records) = std::fs::read_to_string(&verified) else {
                return false;
            };
            keys.iter().enumerate().skip(1).all(|(index, key)| {
                let group = records.lines().find_map(|line| {
                    let mut fields = line.split_whitespace();
                    let candidate = fields.next()?;
                    let _pid = fields.next()?;
                    let pgid = fields.next()?.parse::<u32>().ok()?;
                    (candidate == key).then_some(pgid)
                });
                let Some(pgid) = group else {
                    return false;
                };
                let output = scene
                    .runner
                    .join(format!("velnor/state-select/u-r{index}/out"));
                let output_ready = std::fs::read_to_string(output)
                    .is_ok_and(|contents| contents.lines().any(|candidate| candidate == key));
                output_ready
                    && !group_is_live(pgid)
                    && process_is_reaped(pgid)
                    && !state_dir.join(format!("status-u{index}")).exists()
                    && !state_dir.join(format!("release-{pgid}")).exists()
            })
        },
        Duration::from_secs(60),
    )
}

#[test]
#[cfg(target_os = "linux")]
fn state_v4_select_many_units_reaps_a_hung_verifiers_descendants() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-many-hung", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let roots: Vec<String> = (0..65).map(|ordinal| format!("u{ordinal:02}")).collect();
    let units: Vec<(StateUnit, &str)> = roots
        .iter()
        .map(|root| (StateUnit::Union, root.as_str()))
        .collect();
    let keys = install_many_unit_candidates(&gh, &roots)?;
    let clock = SupervisorClock::install(&scene, &gh)?;
    let real_python = clock
        .real_python()
        .to_str()
        .ok_or_else(|| fail("invalid Python path"))?;
    let mut selector = RunningSelector::new(spawn_select_units(
        &scene,
        &gh,
        &units,
        &[
            ("VELNOR_STATE_GH_TIMEOUT", "120"),
            ("VELNOR_STATE_UNIT_TIMEOUT", "90"),
            ("VELNOR_STATE_SELECT_DEADLINE", "180"),
            ("FAKE_REAL_PYTHON3", real_python),
        ],
    )?);
    wait_for_rendezvous(&gh, STATE_SEAL_SELECT_MAX_PARALLELISM)?;
    wait_for_healthy_units_clean(&scene, &gh, &keys)?;
    let hung = gh.hung_pids()?;
    if hung.len() < 2 || hung.iter().any(|pid| !process_is_running(*pid)) {
        return Err(fail(
            "the hung verifier descendants exited before unit expiry",
        ));
    }
    let hung_group = process_group(hung[0]).ok_or_else(|| fail("missing hung process group"))?;
    if !group_is_live(hung_group) {
        return Err(fail("the hung verifier group exited before unit expiry"));
    }
    clock.advance("unit-timeout", 91)?;
    clock.wait_for_advance("unit-timeout", Duration::from_secs(10))?;
    if !selector.wait(Duration::from_secs(60))?.success() {
        return Err(fail("unit expiry failed the selector step"));
    }

    assert_many_unit_selections(&scene, &gh, &roots, &keys)?;
    scene.finish()
}

#[test]
#[cfg(target_os = "linux")]
fn selector_does_not_accept_a_status_first_observed_after_unit_expiry() -> Result<(), Box<dyn Error>>
{
    for (label, advance_seconds) in [("equal", 1), ("late", 2)] {
        let scene = Scene::new("seal-select-late-unit", crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let completed = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
        let delayed = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
        gh.list(&[(T3, MAIN, &completed), (T1, MAIN, &delayed)])?;
        gh.attest(&[
            Attestation::signed(&completed),
            Attestation::signed(&delayed),
        ])?;
        let clock = SupervisorClock::install(&scene, &gh)?;
        let real_python = clock
            .real_python()
            .to_str()
            .ok_or_else(|| fail("invalid Python path"))?;
        let gate = PathBuf::from(gh.dir()).join("verify-release");
        let started = PathBuf::from(gh.dir()).join("verify-started");
        let gate_value = gate.to_string_lossy().into_owned();
        let started_value = started.to_string_lossy().into_owned();
        let mut selector = RunningSelector::new(spawn_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, "."), (StateUnit::Union, "b")],
            &[
                ("VELNOR_STATE_GH_TIMEOUT", "30"),
                ("VELNOR_STATE_UNIT_TIMEOUT", "1"),
                ("VELNOR_STATE_SELECT_DEADLINE", "10"),
                ("FAKE_REAL_PYTHON3", real_python),
                ("FAKE_GH_VERIFY_GATE", &gate_value),
                ("FAKE_GH_VERIFY_GATE_KEY", &delayed),
                ("FAKE_GH_VERIFY_STARTED", &started_value),
            ],
        )?);
        wait_until(|| started.exists(), Duration::from_secs(10))?;
        let completed_output = scene.runner.join("velnor/state-select/u-r0/out");
        let verified = PathBuf::from(gh.dir()).join("verified");
        let state_dir = scene.runner.join("velnor/state-select");
        wait_until(
            || {
                let Ok(records) = fs::read_to_string(&verified) else {
                    return false;
                };
                let Some(pgid) = records.lines().find_map(|line| {
                    let mut fields = line.split_whitespace();
                    let candidate = fields.next()?;
                    let _pid = fields.next()?;
                    let group = fields.next()?.parse::<u32>().ok()?;
                    (candidate == completed).then_some(group)
                }) else {
                    return false;
                };
                fs::read_to_string(&completed_output)
                    .is_ok_and(|contents| contents.lines().any(|candidate| candidate == completed))
                    && !group_is_live(pgid)
                    && process_is_reaped(pgid)
                    && !state_dir.join("status-u0").exists()
            },
            Duration::from_secs(20),
        )?;
        let stop = Command::new("kill")
            .args(["-STOP", &selector.child.id().to_string()])
            .status()?;
        if !stop.success() {
            return Err(fail("could not stop the selector at the external boundary"));
        }
        clock.advance(label, advance_seconds)?;
        fs::write(&gate, "release\n")?;
        wait_until(
            || state_dir.join("status-u1").exists(),
            Duration::from_secs(10),
        )?;
        let resume = Command::new("kill")
            .args(["-CONT", &selector.child.id().to_string()])
            .status()?;
        if !resume.success() {
            return Err(fail(
                "could not resume the selector after delayed completion",
            ));
        }
        clock.wait_for_advance(label, Duration::from_secs(10))?;
        if !selector.wait(Duration::from_secs(10))?.success() {
            return Err(fail("unit expiry failed the selector step"));
        }
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            std::slice::from_ref(&completed),
            "completed neighbor survives late status",
        )?;
        expect_flag(
            &produce_flag(&scene, StateUnit::Union, ".")?,
            "false",
            "completed neighbor",
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, "b")?,
            &[],
            "late status remains cold",
        )?;
        expect_flag(
            &produce_flag(&scene, StateUnit::Union, "b")?,
            "true",
            "late status",
        )?;
        scene.finish()?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn assert_many_unit_selections(
    scene: &Scene,
    gh: &Gh,
    roots: &[String],
    keys: &[String],
) -> Result<(), Box<dyn Error>> {
    expect(
        &selected_unit(scene, StateUnit::Union, &roots[0])?,
        &[],
        "hung unit stays cold",
    )?;
    expect_flag(
        &produce_flag(scene, StateUnit::Union, &roots[0])?,
        "true",
        "hung unit",
    )?;
    for (index, root) in roots.iter().enumerate().skip(1) {
        expect(
            &selected_unit(scene, StateUnit::Union, root)?,
            std::slice::from_ref(&keys[index]),
            &format!("healthy unit {root}"),
        )?;
        expect_flag(&produce_flag(scene, StateUnit::Union, root)?, "false", root)?;
    }
    if !std::path::Path::new(&gh.dir())
        .join("healthy-while-hung")
        .exists()
    {
        return Err(fail(
            "healthy verification did not finish while the hung descendant was alive",
        ));
    }
    let rendezvous_count = fs::read_dir(std::path::Path::new(&gh.dir()).join("rv"))?.count();
    if rendezvous_count < STATE_SEAL_SELECT_MAX_PARALLELISM {
        return Err(fail(
            "the selector did not reach the configured concurrency",
        ));
    }
    if gh.peak()? > STATE_SEAL_SELECT_MAX_PARALLELISM {
        return Err(fail("the selector exceeded its configured concurrency"));
    }
    hung_processes_gone(gh)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_many_unit_candidates(gh: &Gh, roots: &[String]) -> Result<Vec<String>, Box<dyn Error>> {
    let hung = unit_key(StateUnit::Union, &roots[0], 0, 0x1000, 1)?;
    let mut rows = vec![(at(0), hung.clone())];
    let mut attested = Vec::new();
    let mut keys = vec![hung.clone()];
    for (index, root) in roots.iter().enumerate().skip(1) {
        let key = unit_key(
            StateUnit::Union,
            root,
            index as u64,
            0x1000 + index as u64,
            1,
        )?;
        rows.push((at(index), key.clone()));
        attested.push(Attestation::signed(&key));
        keys.push(key);
    }
    attested.push(Attestation::signed(&hung));
    gh.list(&rows_of(&rows))?;
    gh.attest(&attested)?;
    hang_keys(gh, &[&hung])?;
    rendezvous(gh, STATE_SEAL_SELECT_MAX_PARALLELISM)?;
    Ok(keys)
}
