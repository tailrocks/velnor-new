// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::error::Error;

#[path = "gh_cache_fake.rs"]
mod gh_cache_fake;

use gh_cache_fake::{GhFake, Ledger};

const MAIN: &str = "refs/heads/main";

fn hash(c: char) -> String {
    c.to_string().repeat(64)
}

fn state_key(trust: &str, unit_token: &str, compat: char, payload: char) -> String {
    format!(
        "velnor-v5-state-{trust}-{unit_token}-s256-{}-s256-{}-s256-{}",
        hash(compat),
        hash('5'),
        hash(payload)
    )
}

fn trusted(unit: &str, root: &str, compat: char, payload: char) -> String {
    state_key("trusted", &format!("{unit}-{root}"), compat, payload)
}

fn sorted(mut ids: Vec<u32>) -> Vec<u32> {
    ids.sort_unstable();
    ids
}

#[test]
fn prune_keeps_the_newest_attested_generation_and_everything_newer_per_root()
-> Result<(), Box<dyn Error>> {
    let mut ledger = Ledger::new();
    let g1 = ledger.add(901, MAIN, &trusted("slice", "root", 'a', '1'));
    let junk_old = ledger.add(902, MAIN, "velnor-v5-state-trusted-slice-root-junk");
    let h1 = ledger.add(
        903,
        MAIN,
        &trusted("slice", "crates-velnor-runner", 'a', '1'),
    );
    let g2 = ledger.add(904, MAIN, &trusted("slice", "root", 'b', '2'));
    let g3 = ledger.add(905, MAIN, &trusted("slice", "root", 'c', '3'));
    let h2 = ledger.add(
        906,
        MAIN,
        &trusted("slice", "crates-velnor-runner", 'a', '2'),
    );
    ledger.attest(g3);
    ledger.attest(h2);
    let no_payload = ledger.add(
        907,
        MAIN,
        &format!(
            "velnor-v5-state-trusted-slice-root-s256-{}-s256-{}",
            hash('c'),
            hash('5')
        ),
    );
    let short_digest = ledger.add(
        908,
        MAIN,
        &format!(
            "velnor-v5-state-trusted-slice-root-s256-{}-s256-{}-s256-{}",
            hash('c'),
            hash('5'),
            "d".repeat(63)
        ),
    );
    let lone_suite = ledger.add(909, MAIN, &trusted("suite", "lonely", 'a', '1'));
    let only_incomplete = ledger.add(
        910,
        MAIN,
        &format!(
            "velnor-v5-state-trusted-suite-incomplete-s256-{}",
            hash('a')
        ),
    );
    let fake = GhFake::new(&ledger)?;
    let run = fake.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert_eq!(
        sorted(run.attempted),
        sorted(vec![g1, g2, h1]),
        "kept must be g3={g3} h2={h2} and every unparseable key ({junk_old}, {no_payload}, {short_digest}, {only_incomplete}, {lone_suite})"
    );
    Ok(())
}

#[test]
fn prune_groups_by_trust_unit_root_only() -> Result<(), Box<dyn Error>> {
    let mut ledger = Ledger::new();
    let x1 = ledger.add(801, MAIN, &trusted("slice", "x", 'a', '1'));
    ledger.add(802, MAIN, &trusted("suite", "x", 'a', '1'));
    ledger.add(
        803,
        MAIN,
        &format!("velnor-v2-sources-trusted-s256-{}", hash('a')),
    );
    ledger.add(804, MAIN, &format!("mise-tools-v2-{}", hash('a')));
    ledger.add(805, MAIN, &state_key("pr", "slice-x", 'a', '1'));
    ledger.add(806, MAIN, &state_key("pr", "slice-x", 'b', '2'));
    let y1 = ledger.add(807, MAIN, &trusted("slice", "y", 'a', '1'));
    ledger.add(808, "refs/pull/7/merge", &trusted("slice", "x", 'a', '3'));
    ledger.add(809, "refs/pull/7/merge", &trusted("slice", "y", 'a', '3'));
    let x2 = ledger.add(810, MAIN, &trusted("slice", "x", 'b', '2'));
    let y2 = ledger.add(811, MAIN, &trusted("slice", "y", 'b', '2'));
    ledger.attest(x2);
    ledger.attest(y2);
    let hostile_x = format!(
        "velnor-v5-state-trusted-slice-s256-x-s256-{}-s256-{}-s256-{}",
        hash('f'),
        hash('5'),
        hash('f')
    );
    let hostile_y = hostile_x.replace("slice-s256-x", "slice-s256-y");
    ledger.add(812, MAIN, &hostile_x);
    ledger.add(813, MAIN, &hostile_y);
    ledger.add(814, "refs/pull/7/merge", &trusted("slice", "x", 'c', '4'));
    ledger.add(815, "refs/pull/7/merge", &trusted("slice", "y", 'c', '4'));
    let fake = GhFake::new(&ledger)?;
    let run = fake.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert_eq!(
        sorted(run.attempted),
        sorted(vec![x1, y1]),
        "only the superseded slice/x and slice/y generations on main may go; kept x2={x2} y2={y2}"
    );
    Ok(())
}

#[test]
fn prune_warns_and_exits_zero_on_gh_failure() -> Result<(), Box<dyn Error>> {
    let mut ledger = Ledger::new();
    let a1 = ledger.add(701, MAIN, &trusted("slice", "root", 'a', '1'));
    let a2 = ledger.add(702, MAIN, &trusted("slice", "root", 'b', '2'));
    let a3 = ledger.add(703, MAIN, &trusted("slice", "root", 'c', '3'));
    ledger.attest(a3);
    let list_down = GhFake::new(&ledger)?.failing_list();
    let run = list_down.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert!(
        run.stdout
            .contains("::warning::cache_unavailable: could not list velnor-v5-state-trusted-"),
        "stdout: {}",
        run.stdout
    );
    assert!(run.attempted.is_empty(), "deleted {:?}", run.attempted);

    let delete_down = GhFake::new(&ledger)?.failing_delete(&[a2]);
    let run = delete_down.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert_eq!(run.attempted, [a2, a1], "stdout: {}", run.stdout);
    assert!(
        run.stdout.contains(&format!(
            "::warning::cache_unavailable: could not delete {a2}"
        )),
        "stdout: {}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains(&format!("velnor: pruned superseded entry {a1} of")),
        "stdout: {}",
        run.stdout
    );
    Ok(())
}

#[test]
fn retired_state_entries_are_all_pruned_and_current_ones_kept() -> Result<(), Box<dyn Error>> {
    let (a, b, c) = (hash('a'), hash('b'), hash('c'));
    let mut ledger = Ledger::new();
    let retired = [
        ledger.add(1, MAIN, &format!("velnor-v1-state-trusted-s256-{a}")),
        ledger.add(
            2,
            MAIN,
            &format!("velnor-v1-state-trusted-s256-{c}-s256-{b}"),
        ),
        ledger.add(
            11,
            MAIN,
            &format!("velnor-v2-state-trusted-s256-{a}-s256-{b}"),
        ),
        ledger.add(
            12,
            MAIN,
            &format!("velnor-v2-state-trusted-s256-{a}-s256-{c}"),
        ),
        ledger.add(
            13,
            MAIN,
            &format!("velnor-v2-state-trusted-s256-{c}-s256-{b}"),
        ),
        ledger.add(
            21,
            MAIN,
            &format!("velnor-v3-state-trusted-s256-{a}-s256-{b}"),
        ),
        ledger.add(
            22,
            MAIN,
            &format!("velnor-v3-state-trusted-s256-{c}-s256-{b}"),
        ),
    ];
    let v4_key = format!("velnor-v4-state-trusted-union-root-s256-{a}-s256-{b}-s256-{c}");
    let retired_v4 = ledger.add(41, MAIN, &v4_key);
    let mut retired = retired.to_vec();
    retired.push(retired_v4);
    ledger.add(31, MAIN, &trusted("slice", "root", 'a', 'b'));
    let slice_a_older = ledger.add(51, MAIN, &trusted("slice", "a", 'a', '1'));
    let slice_a_b_newer = ledger.add(52, MAIN, &trusted("slice", "a-b", 'a', '2'));
    let cli_newer = ledger.add(53, MAIN, &state_key("trusted", "cli", 'a', '3'));
    ledger.attest(slice_a_b_newer);
    ledger.attest(cli_newer);
    let union_older = ledger.add(61, MAIN, &trusted("union", "root", 'a', '1'));
    let union_newer = ledger.add(62, MAIN, &trusted("union", "root", 'a', '2'));
    let empty_id_older = ledger.add(63, MAIN, &trusted("slice", "", 'a', '1'));
    let empty_id_newer = ledger.add(64, MAIN, &trusted("slice", "", 'a', '2'));
    ledger.attest(union_newer);
    ledger.attest(empty_id_newer);
    let fake = GhFake::new(&ledger)?;
    let run = fake.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert_eq!(
        sorted(run.attempted),
        retired,
        "every retired v1 to v4 entry goes; slice-a ({slice_a_older}) must survive the newer attested slice-a-b ({slice_a_b_newer}) and cli ({cli_newer}); unknown units union-root ({union_older}, {union_newer}) and an empty slice id ({empty_id_older}, {empty_id_newer}) are never grouped"
    );
    Ok(())
}

#[test]
fn prune_deletes_nothing_when_only_the_older_generation_is_attested() -> Result<(), Box<dyn Error>>
{
    let mut ledger = Ledger::new();
    let k1 = ledger.add(301, MAIN, &trusted("slice", "root", 'a', '1'));
    ledger.add(302, MAIN, &trusted("slice", "root", 'b', '2'));
    ledger.add(303, MAIN, &trusted("slice", "root", 'c', '3'));
    ledger.attest(k1);
    let run = GhFake::new(&ledger)?.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert!(
        run.attempted.is_empty(),
        "the attested generation {k1} must survive two unattested successors: deleted {:?}",
        run.attempted
    );
    Ok(())
}

#[test]
fn prune_deletes_only_what_is_older_than_the_newest_attested_generation()
-> Result<(), Box<dyn Error>> {
    let mut ledger = Ledger::new();
    let k1 = ledger.add(301, MAIN, &trusted("slice", "root", 'a', '1'));
    let k2 = ledger.add(302, MAIN, &trusted("slice", "root", 'b', '2'));
    ledger.add(303, MAIN, &trusted("slice", "root", 'c', '3'));
    ledger.add(304, MAIN, &trusted("slice", "root", 'd', '4'));
    ledger.attest(k1);
    ledger.attest(k2);
    let run = GhFake::new(&ledger)?.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert_eq!(
        run.attempted,
        [k1],
        "302 anchors the group; 303 and 304 are newer and stay"
    );
    Ok(())
}

#[test]
fn prune_deletes_nothing_in_a_group_without_a_verified_anchor() -> Result<(), Box<dyn Error>> {
    let mut none = Ledger::new();
    for (id, compat) in [(401, 'a'), (402, 'b'), (403, 'c')] {
        none.add(id, MAIN, &trusted("slice", "root", compat, '1'));
    }
    let run = GhFake::new(&none)?.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert!(run.attempted.is_empty(), "deleted {:?}", run.attempted);

    let mut beyond_cap = Ledger::new();
    let oldest = beyond_cap.add(500, MAIN, &trusted("slice", "root", 'a', '1'));
    beyond_cap.attest(oldest);
    for (id, compat) in [(501, 'b'), (502, 'c'), (503, 'd'), (504, 'e'), (505, 'f')] {
        beyond_cap.add(id, MAIN, &trusted("slice", "root", compat, '1'));
    }
    beyond_cap.add(506, MAIN, &trusted("slice", "root", 'a', '2'));
    let run = GhFake::new(&beyond_cap)?.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert!(
        run.attempted.is_empty(),
        "an anchor beyond the fifth entry must not be searched for: deleted {:?}",
        run.attempted
    );
    Ok(())
}
