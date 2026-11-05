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

fn state_key(trust: &str, unit: &str, root: &str, compat: char, payload: char) -> String {
    format!(
        "velnor-v4-state-{trust}-{unit}-{root}-s256-{}-s256-{}-s256-{}",
        hash(compat),
        hash('5'),
        hash(payload)
    )
}

fn trusted(unit: &str, root: &str, compat: char, payload: char) -> String {
    state_key("trusted", unit, root, compat, payload)
}

fn sorted(mut ids: Vec<u32>) -> Vec<u32> {
    ids.sort_unstable();
    ids
}

#[test]
fn prune_keeps_the_newest_attested_generation_and_everything_newer_per_root()
-> Result<(), Box<dyn Error>> {
    let mut ledger = Ledger::new();
    let g1 = ledger.add(901, MAIN, &trusted("union", "root", 'a', '1'));
    let junk_old = ledger.add(902, MAIN, "velnor-v4-state-trusted-union-root-junk");
    let h1 = ledger.add(
        903,
        MAIN,
        &trusted("union", "crates-velnor-runner", 'a', '1'),
    );
    let g2 = ledger.add(904, MAIN, &trusted("union", "root", 'b', '2'));
    let g3 = ledger.add(905, MAIN, &trusted("union", "root", 'c', '3'));
    let h2 = ledger.add(
        906,
        MAIN,
        &trusted("union", "crates-velnor-runner", 'a', '2'),
    );
    ledger.attest(g3);
    ledger.attest(h2);
    let no_payload = ledger.add(
        907,
        MAIN,
        &format!(
            "velnor-v4-state-trusted-union-root-s256-{}-s256-{}",
            hash('c'),
            hash('5')
        ),
    );
    let short_digest = ledger.add(
        908,
        MAIN,
        &format!(
            "velnor-v4-state-trusted-union-root-s256-{}-s256-{}-s256-{}",
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
            "velnor-v4-state-trusted-suite-incomplete-s256-{}",
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
    let x1 = ledger.add(801, MAIN, &trusted("union", "x", 'a', '1'));
    ledger.add(802, MAIN, &trusted("suite", "x", 'a', '1'));
    ledger.add(
        803,
        MAIN,
        &format!("velnor-v2-sources-trusted-s256-{}", hash('a')),
    );
    ledger.add(804, MAIN, &format!("mise-tools-v2-{}", hash('a')));
    ledger.add(805, MAIN, &state_key("pr", "union", "x", 'a', '1'));
    ledger.add(806, MAIN, &state_key("pr", "union", "x", 'b', '2'));
    let y1 = ledger.add(807, MAIN, &trusted("union", "y", 'a', '1'));
    ledger.add(808, "refs/pull/7/merge", &trusted("union", "x", 'a', '3'));
    ledger.add(809, "refs/pull/7/merge", &trusted("union", "y", 'a', '3'));
    let x2 = ledger.add(810, MAIN, &trusted("union", "x", 'b', '2'));
    let y2 = ledger.add(811, MAIN, &trusted("union", "y", 'b', '2'));
    ledger.attest(x2);
    ledger.attest(y2);
    let hostile_x = format!(
        "velnor-v4-state-trusted-union-s256-x-s256-{}-s256-{}-s256-{}",
        hash('f'),
        hash('5'),
        hash('f')
    );
    let hostile_y = hostile_x.replace("union-s256-x", "union-s256-y");
    ledger.add(812, MAIN, &hostile_x);
    ledger.add(813, MAIN, &hostile_y);
    ledger.add(814, "refs/pull/7/merge", &trusted("union", "x", 'c', '4'));
    ledger.add(815, "refs/pull/7/merge", &trusted("union", "y", 'c', '4'));
    let fake = GhFake::new(&ledger)?;
    let run = fake.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert_eq!(
        sorted(run.attempted),
        sorted(vec![x1, y1]),
        "only the superseded union/x and union/y generations on main may go; kept x2={x2} y2={y2}"
    );
    Ok(())
}

#[test]
fn prune_warns_and_exits_zero_on_gh_failure() -> Result<(), Box<dyn Error>> {
    let mut ledger = Ledger::new();
    let a1 = ledger.add(701, MAIN, &trusted("union", "root", 'a', '1'));
    let a2 = ledger.add(702, MAIN, &trusted("union", "root", 'b', '2'));
    let a3 = ledger.add(703, MAIN, &trusted("union", "root", 'c', '3'));
    ledger.attest(a3);
    let list_down = GhFake::new(&ledger)?.failing_list();
    let run = list_down.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert!(
        run.stdout
            .contains("::warning::cache_unavailable: could not list velnor-v4-state-trusted-"),
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
    ledger.add(31, MAIN, &trusted("union", "root", 'a', 'b'));
    let fake = GhFake::new(&ledger)?;
    let run = fake.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert_eq!(
        sorted(run.attempted),
        retired,
        "retired v1, v2 and v3 state entries must all be deleted and v4 kept"
    );
    Ok(())
}

#[test]
fn prune_deletes_nothing_when_only_the_older_generation_is_attested() -> Result<(), Box<dyn Error>>
{
    let mut ledger = Ledger::new();
    let k1 = ledger.add(301, MAIN, &trusted("union", "root", 'a', '1'));
    ledger.add(302, MAIN, &trusted("union", "root", 'b', '2'));
    ledger.add(303, MAIN, &trusted("union", "root", 'c', '3'));
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
    let k1 = ledger.add(301, MAIN, &trusted("union", "root", 'a', '1'));
    let k2 = ledger.add(302, MAIN, &trusted("union", "root", 'b', '2'));
    ledger.add(303, MAIN, &trusted("union", "root", 'c', '3'));
    ledger.add(304, MAIN, &trusted("union", "root", 'd', '4'));
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
        none.add(id, MAIN, &trusted("union", "root", compat, '1'));
    }
    let run = GhFake::new(&none)?.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert!(run.attempted.is_empty(), "deleted {:?}", run.attempted);

    let mut beyond_cap = Ledger::new();
    let oldest = beyond_cap.add(500, MAIN, &trusted("union", "root", 'a', '1'));
    beyond_cap.attest(oldest);
    for (id, compat) in [(501, 'b'), (502, 'c'), (503, 'd'), (504, 'e'), (505, 'f')] {
        beyond_cap.add(id, MAIN, &trusted("union", "root", compat, '1'));
    }
    beyond_cap.add(506, MAIN, &trusted("union", "root", 'a', '2'));
    let run = GhFake::new(&beyond_cap)?.run_prune(MAIN)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert!(
        run.attempted.is_empty(),
        "an anchor beyond the fifth entry must not be searched for: deleted {:?}",
        run.attempted
    );
    Ok(())
}

#[test]
fn prune_targets_the_completed_runs_branch_not_the_workflow_ref() -> Result<(), Box<dyn Error>> {
    const FEATURE: &str = "refs/heads/design/interaction-polish";
    let mut ledger = Ledger::new();
    let f1 = ledger.add(601, FEATURE, &trusted("union", "root", 'a', '1'));
    let f2 = ledger.add(602, FEATURE, &trusted("union", "root", 'b', '2'));
    let f3 = ledger.add(603, FEATURE, &trusted("union", "root", 'c', '3'));
    ledger.add(611, MAIN, &trusted("union", "root", 'a', '1'));
    ledger.add(612, MAIN, &trusted("union", "root", 'b', '2'));
    let main_head = ledger.add(613, MAIN, &trusted("union", "root", 'c', '3'));
    ledger.attest(f3);
    ledger.attest(main_head);
    let run = GhFake::new(&ledger)?.run_prune(FEATURE)?;
    assert_eq!(run.status, Some(0), "stdout: {}", run.stdout);
    assert_eq!(
        run.attempted,
        [f2, f1],
        "only the completed run's branch is pruned: stdout {}",
        run.stdout
    );
    Ok(())
}

fn assert_head_branch_refused(fake: &GhFake, head_branch: &str) -> Result<(), Box<dyn Error>> {
    let run = fake.run_prune_for(head_branch)?;
    assert_ne!(run.status, Some(0), "accepted {head_branch:?}");
    assert!(run.attempted.is_empty(), "deleted {:?}", run.attempted);
    assert!(
        run.stderr.contains("cache_prune_head_branch_invalid"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

#[test]
fn prune_refuses_a_head_branch_that_could_inject_shell() -> Result<(), Box<dyn Error>> {
    let mut ledger = Ledger::new();
    ledger.add(701, MAIN, &trusted("union", "root", 'a', '1'));
    let fake = GhFake::new(&ledger)?;
    for hostile in ["main\"; touch pwned; \"", "$(touch pwned)", "main x", ""] {
        assert_head_branch_refused(&fake, hostile)?;
    }
    Ok(())
}

#[test]
fn prune_refuses_a_non_ascii_head_branch_in_a_utf8_locale() -> Result<(), Box<dyn Error>> {
    let mut ledger = Ledger::new();
    ledger.add(701, MAIN, &trusted("union", "root", 'a', '1'));
    let fake = GhFake::new(&ledger)?.in_locale("en_US.UTF-8");
    for hostile in ["\u{e9}", "stra\u{df}e"] {
        assert_head_branch_refused(&fake, hostile)?;
    }
    Ok(())
}
