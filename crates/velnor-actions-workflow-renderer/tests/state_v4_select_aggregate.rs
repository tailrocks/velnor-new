// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use velnor_actions_contract::workflow::cache_family::StateUnit;

use crate::gc1_scene::{Scene, fail};
#[cfg(target_os = "linux")]
use crate::gc1_seal_clock::SupervisorClock;
use crate::gc1_seal_support::{Attestation, Gh, MAIN, check_selector_markers};
use crate::gc1_seal_units::{
    expect, expect_flag, hang_keys, hung_processes_gone, produce_flag, rendezvous, rows_of,
    selected_unit, spawn_select_units, unit_key,
};
use crate::gc1_seal_units::{group_is_live, process_group, process_is_reaped};

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

fn wait_for_clean_success(
    scene: &Scene,
    gh: &Gh,
    unit_index: usize,
    key: &str,
) -> Result<(), Box<dyn Error>> {
    let dir = PathBuf::from(gh.dir());
    let output = scene
        .runner
        .join(format!("velnor/state-select/u-r{unit_index}/out"));
    let verified = dir.join("verified");
    let state_dir = scene.runner.join("velnor/state-select");
    let mut group = None;
    wait_until(
        || {
            if group.is_none() {
                let Ok(contents) = fs::read_to_string(&verified) else {
                    return false;
                };
                group = contents.lines().find_map(|line| {
                    let mut fields = line.split_whitespace();
                    let candidate = fields.next()?;
                    let _pid = fields.next()?;
                    let pgid = fields.next()?.parse::<u32>().ok()?;
                    (candidate == key).then_some(pgid)
                });
            }
            let output_ready = fs::read_to_string(&output)
                .is_ok_and(|contents| contents.lines().any(|candidate| candidate == key));
            output_ready
                && group.is_some_and(|pgid| {
                    !group_is_live(pgid)
                        && process_is_reaped(pgid)
                        && !state_dir.join(format!("status-u{unit_index}")).exists()
                        && !state_dir.join(format!("release-{pgid}")).exists()
                })
        },
        Duration::from_secs(20),
    )
}

fn wait_for_rendezvous(gh: &Gh, expected: usize) -> Result<(), Box<dyn Error>> {
    let markers = PathBuf::from(gh.dir()).join("rv");
    wait_until(
        || fs::read_dir(&markers).is_ok_and(|entries| entries.count() >= expected),
        Duration::from_secs(20),
    )
}

fn aggregate_candidates(gh: &Gh, roots: &[String]) -> Result<Vec<String>, Box<dyn Error>> {
    let keys = roots
        .iter()
        .enumerate()
        .map(|(index, root)| {
            unit_key(
                StateUnit::Union,
                root,
                index as u64,
                0x1000 + index as u64,
                1,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rows = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (crate::gc1_seal_units::at(index), key.clone()))
        .collect::<Vec<_>>();
    let attested = keys
        .iter()
        .map(|key| Attestation::signed(key))
        .collect::<Vec<_>>();
    gh.list(&rows_of(&rows))?;
    gh.attest(&attested)?;
    let hanging = keys.iter().skip(1).map(String::as_str).collect::<Vec<_>>();
    hang_keys(gh, &hanging)?;
    rendezvous(gh, 32)?;
    Ok(keys)
}

#[test]
#[cfg(target_os = "linux")]
fn aggregate_expiry_keeps_only_completed_cleaned_units() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new(
        "seal-select-aggregate-preserves",
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    let roots: Vec<String> = (1..65).map(|ordinal| format!("r{ordinal:02}")).collect();
    let roots = std::iter::once(".".to_owned())
        .chain(roots)
        .collect::<Vec<_>>();
    let targets = roots
        .iter()
        .map(|root| (StateUnit::Union, root.as_str()))
        .collect::<Vec<_>>();
    let keys = aggregate_candidates(&gh, &roots)?;
    let clock = SupervisorClock::install(&scene, &gh)?;
    let real_python = clock
        .real_python()
        .to_str()
        .ok_or_else(|| fail("invalid Python path"))?;

    let mut selector = RunningSelector::new(spawn_select_units(
        &scene,
        &gh,
        &targets,
        &[
            ("VELNOR_STATE_GH_TIMEOUT", "300"),
            ("VELNOR_STATE_UNIT_TIMEOUT", "240"),
            ("VELNOR_STATE_SELECT_DEADLINE", "120"),
            ("FAKE_REAL_PYTHON3", real_python),
        ],
    )?);
    wait_for_rendezvous(&gh, 32)?;
    wait_for_clean_success(&scene, &gh, 0, &keys[0])?;
    wait_until(
        || gh.hung_pids().is_ok_and(|pids| pids.len() >= 2),
        Duration::from_secs(5),
    )?;
    let hung_pids = gh.hung_pids()?;
    let hung_group =
        process_group(hung_pids[0]).ok_or_else(|| fail("missing hung process group"))?;
    if !group_is_live(hung_group) {
        return Err(fail(
            "the unfinished verifier group exited before aggregate expiry",
        ));
    }
    let unlaunched = scene
        .runner
        .join(format!("velnor/state-select/u-r{}/part", targets.len() - 1));
    if unlaunched.exists() {
        return Err(fail("a queued unit started before aggregate expiry"));
    }
    if gh.peak()? > 32 {
        return Err(fail("the selector exceeded its configured concurrency"));
    }
    clock.advance("aggregate-expiry", 121)?;
    clock.wait_for_advance("aggregate-expiry", Duration::from_secs(10))?;
    let status = selector.wait(Duration::from_secs(30))?;
    check_selector_markers(&gh)?;
    if !status.success() {
        return Err(fail("aggregate expiry failed the selector step"));
    }

    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&keys[0]),
        "completed unit survives aggregate expiry",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "false",
        "completed unit",
    )?;
    for root in roots.iter().skip(1) {
        expect(
            &selected_unit(&scene, StateUnit::Union, root)?,
            &[],
            &format!("unfinished unit {root}"),
        )?;
        expect_flag(&produce_flag(&scene, StateUnit::Union, root)?, "true", root)?;
    }
    hung_processes_gone(&gh)?;
    scene.finish()
}

#[test]
#[cfg(target_os = "linux")]
fn failed_partial_unit_stays_cold_and_neighbor_survives() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-partial-failure", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let first = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let failed = unit_key(StateUnit::Union, ".", 0, 0x1001, 2)?;
    let neighbor = unit_key(StateUnit::Union, "b", 1, 0x1002, 3)?;
    gh.list(&[
        ("2026-11-04T03:00:00Z", MAIN, &first),
        ("2026-11-04T02:00:00Z", MAIN, &failed),
        ("2026-11-04T01:00:00Z", MAIN, &neighbor),
    ])?;
    gh.attest(&[
        Attestation::signed(&first),
        Attestation::signed(&failed),
        Attestation::signed(&neighbor),
    ])?;
    gh.fail_keys(&[&failed])?;
    crate::gc1_seal_units::run_select_units(
        &scene,
        &gh,
        &[(StateUnit::Union, "."), (StateUnit::Union, "b")],
        &[],
    )?;

    let part = scene.runner.join("velnor/state-select/u-r0/part");
    let partial = fs::read_to_string(part)?;
    if partial.trim() != first {
        return Err(fail("the unit failure did not leave a partial candidate"));
    }
    if !Path::new(&gh.dir())
        .join(format!("failed-{failed}"))
        .exists()
    {
        return Err(fail(
            "the fake GH did not return 124 for the later candidate",
        ));
    }
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        &[],
        "failed unit remains cold",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "true",
        "failed unit",
    )?;
    expect(
        &selected_unit(&scene, StateUnit::Union, "b")?,
        std::slice::from_ref(&neighbor),
        "healthy neighbor survives unit failure",
    )?;
    scene.finish()
}
