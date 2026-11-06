// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use velnor_actions_contract::workflow::cache_family::StateUnit;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_clock::SupervisorClock;
use crate::gc1_seal_support::{Attestation, Gh};
use crate::gc1_seal_units::{
    expect, expect_flag, produce_flag, run_select_units, selected_unit, unit_key,
};
use call_helpers::{
    assert_listing_group_reaped, await_budget_boundary, clear_readiness_path,
    spawn_configured_listing_selector, wait_for_live_listing_pid,
};

#[path = "state_v4_select_call_helpers.rs"]
mod call_helpers;

#[path = "state_v4_select_pid_publication.rs"]
mod pid_publication;

#[path = "state_v4_select_aggregate_cleanup.rs"]
mod aggregate_cleanup;
#[path = "state_v4_select_aggregate_launch.rs"]
mod aggregate_launch;
#[path = "state_v4_select_aggregate_observation.rs"]
mod aggregate_observation;
#[path = "state_v4_select_call_numeric.rs"]
mod call_numeric;
#[path = "state_v4_select_listing_status.rs"]
mod listing_status;
#[path = "state_v4_select_verifier_bounds.rs"]
mod verifier_bounds;

struct RunningSelector {
    child: Child,
    finished: bool,
    gh_dir: PathBuf,
}

impl RunningSelector {
    fn new(child: Child, gh: &Gh) -> Self {
        Self {
            child,
            finished: false,
            gh_dir: PathBuf::from(gh.dir()),
        }
    }

    fn observe(&mut self, timeout: Duration) -> Result<Option<ExitStatus>, Box<dyn Error>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.finished = true;
                eprintln!("X101 selector completion=observe");
                crate::gc1_seal_support::check_selector_markers_at(&self.gh_dir)?;
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait(&mut self, timeout: Duration) -> Result<ExitStatus, Box<dyn Error>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.finished = true;
                eprintln!("X101 selector completion=wait");
                crate::gc1_seal_support::check_selector_markers_at(&self.gh_dir)?;
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(fail("selector exceeded its real-time watchdog"));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

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
            while crate::gc1_seal_units::group_is_live(self.child.id()) && Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(10));
            }
            if crate::gc1_seal_units::group_is_live(self.child.id()) {
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

#[test]
fn listing_call_deadline_cleans_group_before_cold_publication() -> Result<(), Box<dyn Error>> {
    run_listing_call_deadline_case("equal", 2)?;
    run_listing_call_deadline_case("late", 3)
}

fn run_listing_call_deadline_case(label: &str, advance: u64) -> Result<(), Box<dyn Error>> {
    let scene = Scene::new(
        &format!("seal-select-list-call-{label}"),
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    gh.list(&crate::gc1_seal_units::rows_of(&[(
        crate::gc1_seal_units::at(0),
        key.clone(),
    )]))?;
    gh.attest(&[Attestation::signed(&key)])?;
    gh.mode("list-hang")?;
    let clock = SupervisorClock::install(&scene, &gh)?;
    let real_python = clock
        .real_python()
        .to_str()
        .ok_or_else(|| fail("invalid Python path"))?;
    let gh_dir = PathBuf::from(gh.dir());
    let ready_path = gh_dir.join(format!("list-ready-{label}"));
    clear_readiness_path(&ready_path)?;
    let ready_path_value = ready_path
        .to_str()
        .ok_or_else(|| fail("invalid listing readiness path"))?;
    let mut selector = RunningSelector::new(
        crate::gc1_seal_units::spawn_select_units(
            &scene,
            &gh,
            &[(StateUnit::Union, ".")],
            &[
                ("VELNOR_STATE_GH_TIMEOUT", "2"),
                ("VELNOR_STATE_UNIT_TIMEOUT", "20"),
                ("VELNOR_STATE_SELECT_DEADLINE", "30"),
                ("FAKE_REAL_PYTHON3", real_python),
                ("FAKE_SUPERVISOR_CLOCK_FREEZE", "1"),
                ("FAKE_GH_LIST_PID_READY", ready_path_value),
            ],
        )?,
        &gh,
    );
    let (list_pid, list_group) = wait_for_live_listing_pid(
        &ready_path,
        &gh_dir.join("list-pid"),
        Duration::from_secs(10),
    )?;
    clock.advance(label, advance)?;
    clock.wait_for_advance(label, Duration::from_secs(10))?;
    let early = selector.observe(Duration::from_secs(3))?;
    let status = if let Some(status) = early {
        status
    } else {
        clock.advance("listing-cleanup", 30)?;
        selector.wait(Duration::from_secs(10))?
    };
    if early.is_none() {
        return Err(fail("listing did not complete at the call deadline"));
    }
    if !status.success() {
        return Err(fail("listing expiry failed the selector step"));
    }
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        &[],
        "listing call deadline",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "true",
        "listing call deadline",
    )?;
    if gh_dir.join("verify-pids").exists() {
        return Err(fail("verification started after listing call expiry"));
    }
    assert_listing_group_reaped(
        list_pid,
        list_group,
        "listing process group remained live after publication",
    )?;
    scene.finish()
}

#[test]
fn default_and_override_call_budgets_use_the_configured_deadline() -> Result<(), Box<dyn Error>> {
    run_configured_call_budget_case("default", None, 59, 2)?;
    run_configured_call_budget_case("override-120", Some("120"), 60, 61)?;
    run_configured_call_budget_case("override-300", Some("300"), 120, 181)
}

fn run_configured_call_budget_case(
    label: &str,
    call_limit: Option<&str>,
    before_expiry: u64,
    cross_expiry: u64,
) -> Result<(), Box<dyn Error>> {
    eprintln!("X101 case budget={label}");
    let scene = Scene::new(
        &format!("seal-select-call-budget-{label}"),
        crate::gc1_t53::scripts()?,
    )?;
    let gh = Gh::install(&scene)?;
    gh.mode("list-hang")?;
    let clock = SupervisorClock::install(&scene, &gh)?;
    let (mut selector, gh_dir, ready_path) =
        spawn_configured_listing_selector(&scene, &gh, &clock, label, call_limit)?;
    let (list_pid, list_group) = wait_for_live_listing_pid(
        &ready_path,
        &gh_dir.join("list-pid"),
        Duration::from_secs(10),
    )?;
    let (early_status, _releases) =
        await_budget_boundary(&clock, &mut selector, &gh_dir, label, before_expiry)?;
    if let Some(status) = early_status {
        if !status.success() {
            return Err(fail("configured call expiry failed the selector step"));
        }
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            &[],
            "configured call budget expiry",
        )?;
        assert_listing_group_reaped(
            list_pid,
            list_group,
            "listing process group remained live after configured budget",
        )?;
        scene.finish()?;
        return Err(fail("configured call budget expired before its boundary"));
    }
    let expiry_label = format!("{label}-expiry");
    clock.advance(&expiry_label, cross_expiry)?;
    clock.wait_for_advance(&expiry_label, Duration::from_secs(10))?;
    let early = selector.observe(Duration::from_secs(3))?;
    let status = if let Some(status) = early {
        status
    } else {
        clock.advance("configured-cleanup", 600)?;
        selector.wait(Duration::from_secs(10))?
    };
    if early.is_none() {
        return Err(fail("configured call did not complete at its deadline"));
    }
    if !status.success() {
        return Err(fail("configured call expiry failed the selector step"));
    }
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        &[],
        "configured call budget expiry",
    )?;
    assert_listing_group_reaped(
        list_pid,
        list_group,
        "listing process group remained live after configured budget",
    )?;
    scene.finish()
}

#[test]
fn missing_or_incompatible_gnu_timeout_stays_cold_before_gh() -> Result<(), Box<dyn Error>> {
    for incompatible in [false, true] {
        eprintln!("X101 case incompatible={incompatible}");
        let scene = Scene::new("seal-select-timeout-preflight", crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let key = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
        gh.list(&crate::gc1_seal_units::rows_of(&[(
            crate::gc1_seal_units::at(0),
            key.clone(),
        )]))?;
        gh.attest(&[Attestation::signed(&key)])?;
        let gh_bin = PathBuf::from(gh.dir())
            .parent()
            .ok_or_else(|| fail("fake GH directory has no parent"))?
            .join("bin");
        let path = if incompatible {
            scene.install_shim(
                "timeout",
                "#!/bin/sh\nprintf '%s\\n' called > \"$FAKE_GH_DIR/timeout-called\"\nexit 1\n",
            )?;
            format!("{}:{}", gh_bin.display(), std::env::var("PATH")?)
        } else {
            let restricted = scene.restricted_path(&[
                "bash",
                "cat",
                "chmod",
                "grep",
                "head",
                "mkdir",
                "mv",
                "printenv",
                "python3",
                "rm",
                "sh",
                "sleep",
                "sort",
                "tr",
                "wc",
                "ps",
                "ls",
                "awk",
                "sha256sum",
            ])?;
            format!("{}:{restricted}", gh_bin.display())
        };
        fs::write(PathBuf::from(gh.dir()).join("invocations"), "")?;
        run_select_units(&scene, &gh, &[(StateUnit::Union, ".")], &[("PATH", &path)])?;
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            &[],
            "GNU timeout preflight",
        )?;
        expect_flag(
            &produce_flag(&scene, StateUnit::Union, ".")?,
            "true",
            "GNU timeout preflight",
        )?;
        if !fs::read_to_string(PathBuf::from(gh.dir()).join("invocations"))?.is_empty() {
            return Err(fail("GH ran before GNU timeout preflight passed"));
        }
        if incompatible && !PathBuf::from(gh.dir()).join("timeout-called").exists() {
            return Err(fail("the incompatible timeout fixture was not resolved"));
        }
        scene.finish()?;
    }
    Ok(())
}

fn wait_until(
    mut predicate: impl FnMut() -> bool,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(fail("fixture did not reach its expected state"))
}
