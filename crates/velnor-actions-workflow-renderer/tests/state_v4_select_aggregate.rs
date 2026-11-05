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
use crate::gc1_seal_support::{Attestation, Gh, MAIN};
#[cfg(target_os = "linux")]
use crate::gc1_seal_units::{SupervisorClock, group_is_live, process_group, process_is_reaped};
use crate::gc1_seal_units::{
    expect, expect_flag, hang_keys, hung_processes_gone, produce_flag, rendezvous, rows_of,
    selected_unit, spawn_select_units, unit_key,
};

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

fn install_pid_shim(scene: &Scene, gh: &Gh) -> Result<(String, PathBuf), Box<dyn Error>> {
    let python = PathBuf::from(scene.restricted_path(&["python3"])?).join("python3");
    let pid_file = PathBuf::from(gh.dir()).join("supervisor-pid");
    scene.install_shim(
        "python3",
        r#"#!/bin/sh
exec "$FAKE_REAL_PYTHON3" -I -c '
import os, sys
with open(os.environ["FAKE_SUPERVISOR_PID_FILE"], "w", encoding="utf-8") as pid_file:
    pid_file.write(str(os.getpid()))
exec(compile(sys.stdin.read(), "<selector>", "exec"), {"__name__": "__main__"})
'
"#,
    )?;
    Ok((pid_file.display().to_string(), python))
}

fn send_signal(signal: &str, pid: &str) -> Result<(), Box<dyn Error>> {
    let status = Command::new("kill").args(["-s", signal, pid]).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(fail(&format!("could not send {signal} to selector")))
    }
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
    if !selector.wait(Duration::from_secs(30))?.success() {
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

#[test]
#[cfg(target_os = "linux")]
fn selector_cancellation_cleans_and_propagates_each_signal() -> Result<(), Box<dyn Error>> {
    for (signal, status) in [("HUP", 129), ("INT", 130), ("TERM", 143)] {
        let scene = Scene::new("seal-select-cancellation", crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let completed = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
        let hanging = unit_key(StateUnit::Union, "b", 1, 0x1001, 2)?;
        gh.list(&[
            ("2026-11-04T03:00:00Z", MAIN, &completed),
            ("2026-11-04T02:00:00Z", MAIN, &hanging),
        ])?;
        gh.attest(&[
            Attestation::signed(&completed),
            Attestation::signed(&hanging),
        ])?;
        hang_keys(&gh, &[&hanging])?;
        rendezvous(&gh, 2)?;
        let (pid_file, real_python) = install_pid_shim(&scene, &gh)?;
        let signal_marker = PathBuf::from(gh.dir()).join("cleanup-started");
        let signal_ack = PathBuf::from(gh.dir()).join("cleanup-release");
        let pid_file_env = pid_file.clone();
        let signal_marker_env = signal_marker.display().to_string();
        let signal_ack_env = signal_ack.display().to_string();
        let real_python_env = real_python
            .to_str()
            .ok_or_else(|| fail("invalid Python path"))?
            .to_owned();
        let mut selector = RunningSelector::new(spawn_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, "."), (StateUnit::Union, "b")],
            &[
                ("FAKE_SUPERVISOR_PID_FILE", &pid_file_env),
                ("FAKE_REAL_PYTHON3", &real_python_env),
                ("FAKE_GH_SIGNAL_MARKER", &signal_marker_env),
                ("FAKE_GH_SIGNAL_ACK", &signal_ack_env),
            ],
        )?);
        wait_for_rendezvous(&gh, 2)?;
        wait_for_clean_success(&scene, &gh, 0, &completed)?;
        wait_until(
            || gh.hung_pids().is_ok_and(|pids| pids.len() >= 2),
            Duration::from_secs(5),
        )?;
        let pid = fs::read_to_string(&pid_file)?;
        send_signal(signal, pid.trim())?;
        wait_until(|| signal_marker.exists(), Duration::from_secs(5))?;
        send_signal(signal, pid.trim())?;
        fs::write(&signal_ack, "continue\n")?;
        let actual = selector.wait(Duration::from_secs(10))?.code();
        if actual != Some(status) {
            return Err(fail(&format!(
                "{signal} cancellation returned {actual:?}, wanted {status}"
            )));
        }
        for index in 0..2 {
            let output = scene
                .runner
                .join(format!("velnor/state-select/u-r{index}/out"));
            if !fs::read_to_string(output)?.trim().is_empty() {
                return Err(fail("cancellation retained a selected unit"));
            }
        }
        hung_processes_gone(&gh)?;
        scene.finish()?;
    }
    Ok(())
}
