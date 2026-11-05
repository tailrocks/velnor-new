// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::{Child, Command, ExitStatus};
#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

use velnor_actions_contract::workflow::cache_family::StateUnit;
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_LIST_LIMIT, STATE_SEAL_SELECT_LIST_LIMIT, STATE_SEAL_SELECT_MAX_PARALLELISM,
    STATE_SEAL_VERIFY_LIMIT,
};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh, REPO};
#[cfg(target_os = "linux")]
use crate::gc1_seal_units::{
    SupervisorClock, group_is_live, hang_keys, hung_processes_gone, process_group,
    process_is_reaped, process_is_running, spawn_select_units,
};
use crate::gc1_seal_units::{
    at, expect, expect_flag, produce_flag, rendezvous, rows_of, run_select_units, selected_unit,
    unit_key, unit_scope,
};

const T1: &str = "2026-11-04T01:00:00Z";
const T3: &str = "2026-11-04T03:00:00Z";

#[derive(Clone, Copy)]
enum Kind {
    Attested,
    ForgedUnattested,
    ForgedSigner,
}

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
#[cfg(unix)]
fn plan_never_selects_another_units_attestation_for_a_forged_entry() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-units", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;

    let mut units: Vec<(StateUnit, String, Kind)> = vec![
        (StateUnit::Union, ".".to_owned(), Kind::Attested),
        (StateUnit::Union, "a".to_owned(), Kind::ForgedUnattested),
        (StateUnit::Union, "a-b".to_owned(), Kind::Attested),
        (StateUnit::Suite, "a".to_owned(), Kind::Attested),
        (StateUnit::Suite, "a-b".to_owned(), Kind::ForgedSigner),
    ];
    let cycle = [Kind::Attested, Kind::ForgedUnattested, Kind::ForgedSigner];
    for ordinal in 0..35 {
        units.push((
            StateUnit::Union,
            format!("r{ordinal:02}"),
            cycle[ordinal % cycle.len()],
        ));
    }
    if units.len() <= STATE_SEAL_SELECT_MAX_PARALLELISM {
        return Err(fail("the units must outnumber the parallelism"));
    }

    let mut rows: Vec<(String, String)> = Vec::new();
    let mut attested: Vec<Attestation> = Vec::new();
    let mut wanted: Vec<Vec<String>> = Vec::new();
    for (index, (unit, root, kind)) in units.iter().enumerate() {
        let ordinal = index as u64;
        let snapshot = 0x1000 + ordinal;
        match kind {
            Kind::Attested => {
                let entry = unit_key(*unit, root, ordinal, snapshot, 1)?;
                rows.push((T3.to_owned(), entry.clone()));
                attested.push(Attestation::signed(&entry));
                wanted.push(vec![entry]);
            }
            Kind::ForgedUnattested | Kind::ForgedSigner => {
                let forged = unit_key(*unit, root, ordinal, snapshot, 2)?;
                let older = unit_key(*unit, root, ordinal, 0x2000 + ordinal, 3)?;
                rows.push((T3.to_owned(), forged.clone()));
                rows.push((T1.to_owned(), older.clone()));
                attested.push(Attestation::signed(&older));
                if matches!(kind, Kind::ForgedSigner) {
                    attested.push(Attestation {
                        signer: format!("{REPO}/.github/workflows/release.yml"),
                        ..Attestation::signed(&forged)
                    });
                }
                wanted.push(vec![older]);
            }
        }
    }
    gh.list(&rows_of(&rows))?;
    gh.attest(&attested)?;
    rendezvous(&gh, STATE_SEAL_SELECT_MAX_PARALLELISM)?;

    let targets: Vec<(StateUnit, &str)> = units
        .iter()
        .map(|(unit, root, _)| (*unit, root.as_str()))
        .collect();
    run_select_units(&scene, &gh, &targets, &[])?;

    let mut mismatches: Vec<String> = Vec::new();
    for (index, (unit, root, kind)) in units.iter().enumerate() {
        let label = format!("{} {root}", unit_scope(*unit, root)?.prefix());
        let produce = if matches!(kind, Kind::Attested) {
            "false"
        } else {
            "true"
        };
        mismatches.extend(
            expect(&selected_unit(&scene, *unit, root)?, &wanted[index], &label)
                .err()
                .map(|error| error.to_string()),
        );
        mismatches.extend(
            expect_flag(&produce_flag(&scene, *unit, root)?, produce, &label)
                .err()
                .map(|error| error.to_string()),
        );
    }
    if !mismatches.is_empty() {
        return Err(fail(&mismatches.join("\n")));
    }
    scene.finish()
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

fn filler(count: usize, offset: usize) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    (0..count)
        .map(|index| {
            let key = unit_key(
                StateUnit::Union,
                "z",
                0,
                0x3000 + (offset + index) as u64,
                9,
            )?;
            Ok((at(1000 + (offset + index)), key))
        })
        .collect()
}

#[test]
#[cfg(unix)]
fn plan_selects_nothing_when_the_listing_may_be_truncated_or_unavailable()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-truncated", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let units = [(StateUnit::Union, "."), (StateUnit::Union, "b")];
    let first = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
    let second = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
    gh.attest(&[Attestation::signed(&first), Attestation::signed(&second)])?;

    let listing = |total: usize| -> Result<Vec<(String, String)>, Box<dyn Error>> {
        let mut rows = vec![(at(0), first.clone()), (at(1), second.clone())];
        rows.extend(filler(total - rows.len(), 0)?);
        Ok(rows)
    };

    gh.list(&rows_of(&listing(STATE_SEAL_SELECT_LIST_LIMIT - 1)?))?;
    run_select_units(&scene, &gh, &units, &[])?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&first),
        "999 rows, first unit",
    )?;
    expect(
        &selected_unit(&scene, StateUnit::Union, "b")?,
        std::slice::from_ref(&second),
        "999 rows, second unit",
    )?;
    expect_flag(
        &produce_flag(&scene, StateUnit::Union, ".")?,
        "false",
        "999 rows",
    )?;

    gh.list(&rows_of(&listing(STATE_SEAL_SELECT_LIST_LIMIT)?))?;
    run_select_units(&scene, &gh, &units, &[])?;
    for (unit, root) in units {
        expect(
            &selected_unit(&scene, unit, root)?,
            &[],
            &format!("1000 rows, unit {root}"),
        )?;
        expect_flag(
            &produce_flag(&scene, unit, root)?,
            "true",
            &format!("1000 rows, unit {root}"),
        )?;
    }

    gh.list(&rows_of(&listing(STATE_SEAL_SELECT_LIST_LIMIT - 1)?))?;
    for mode in ["fail", "garbage"] {
        gh.mode(mode)?;
        run_select_units(&scene, &gh, &units, &[])?;
        for (unit, root) in units {
            expect(
                &selected_unit(&scene, unit, root)?,
                &[],
                &format!("listing {mode}, unit {root}"),
            )?;
        }
    }
    gh.mode("ok")?;
    run_select_units(&scene, &gh, &units, &[])?;
    expect(
        &selected_unit(&scene, StateUnit::Union, ".")?,
        std::slice::from_ref(&first),
        "listing restored",
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn plan_caps_each_unit_at_its_own_newest_rows() -> Result<(), Box<dyn Error>> {
    let units = [
        (StateUnit::Union, "."),
        (StateUnit::Union, "b"),
        (StateUnit::Union, "a"),
        (StateUnit::Union, "a-b"),
    ];
    for (rank, selects) in [
        (STATE_SEAL_LIST_LIMIT, true),
        (STATE_SEAL_LIST_LIMIT + 1, false),
    ] {
        let scene = Scene::new("seal-select-cap", crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let other = unit_scope(StateUnit::Union, "b")?.prefix();
        let attested = unit_key(StateUnit::Union, ".", 0, 0x1000, 1)?;
        let newest = unit_key(StateUnit::Union, "b", 1, 0x1001, 1)?;
        let nested = unit_key(StateUnit::Union, "a", 2, 0x1002, 1)?;

        let mut rows: Vec<(String, String)> = Vec::new();
        for position in 1..rank {
            let unattested = unit_key(StateUnit::Union, ".", 0, 0x1000, 100 + position as u64)?;
            rows.push((at(2 * position), unattested));
        }
        rows.push((at(2 * rank), attested.clone()));
        rows.push((at(1), newest.clone()));
        for position in 1..=(STATE_SEAL_LIST_LIMIT + 50) {
            rows.push((
                at(2 * position + 1),
                format!("{other}s256-nothex{position}"),
            ));
        }
        let mut sealed = vec![Attestation::signed(&attested), Attestation::signed(&newest)];
        let neighbours: Vec<String> = (0..STATE_SEAL_LIST_LIMIT + 50)
            .map(|position| unit_key(StateUnit::Union, "a-b", 3, 0x1003, 1000 + position as u64))
            .collect::<Result<_, _>>()?;
        for (position, key) in neighbours.iter().enumerate() {
            rows.push((at(600 + position), key.clone()));
            sealed.push(Attestation::signed(key));
        }
        rows.push((at(900), nested.clone()));
        sealed.push(Attestation::signed(&nested));
        gh.list(&rows_of(&rows))?;
        gh.attest(&sealed)?;
        run_select_units(&scene, &gh, &units, &[])?;

        let want: Vec<String> = if selects { vec![attested] } else { Vec::new() };
        expect(
            &selected_unit(&scene, StateUnit::Union, ".")?,
            &want,
            &format!("attested row at rank {rank}"),
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, "b")?,
            &[newest],
            &format!("neighbour unit with rank {rank}"),
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, "a")?,
            &[nested],
            &format!(
                "nested unit under {} newer entries of its neighbour",
                neighbours.len()
            ),
        )?;
        expect(
            &selected_unit(&scene, StateUnit::Union, "a-b")?,
            &neighbours[..STATE_SEAL_VERIFY_LIMIT],
            "nested neighbour keeps its own newest entries",
        )?;
        scene.finish()?;
    }
    Ok(())
}
