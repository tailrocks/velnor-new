// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::Result;

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, git};
use crate::native_setup_velnor_fetch::{Cold, RAN, default_branch_commit, upstream};

const ACTION: &str = "actions/setup-velnor/action.yml";

fn lose_loose_blob(repository: &Path, pin: &str) -> Result<()> {
    let id = git(repository, &["rev-parse", &format!("{pin}:{ACTION}")])?;
    std::fs::remove_file(repository.join("objects").join(&id[..2]).join(&id[2..]))?;
    Ok(())
}

fn warm(rig: &Rig, cold: &Cold<'_>, pin: &str) -> Result<()> {
    let checkout = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(
        checkout.code,
        Some(0),
        "{}\n{}",
        checkout.stdout,
        checkout.stderr
    );
    let ran = cold.run(pin)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(rig.refetches(), 0);
    Ok(())
}

fn assert_fails_naming(ran: &Ran, pin: &str) {
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
    let named = format!("the pin {pin}");
    assert!(
        ran.lines().iter().any(|line| line.contains(&named)),
        "{named}\n{}",
        ran.stdout
    );
    assert!(!ran.has_line(RAN), "{}", ran.stdout);
}

#[test]
fn a_blob_lost_from_a_warm_mirror_is_repaired_once_and_the_next_job_succeeds() -> Result<()> {
    let rig = Rig::new()?;
    let pinned = default_branch_commit(&rig)?;
    let cold = Cold::new(&rig)?;
    warm(&rig, &cold, &pinned)?;
    lose_loose_blob(&rig.mirror(), &pinned)?;
    let repaired = cold.run(&pinned)?;
    assert_eq!(
        repaired.code,
        Some(0),
        "{}\n{}",
        repaired.stdout,
        repaired.stderr
    );
    assert!(repaired.has_line(RAN), "{}", repaired.stdout);
    assert_eq!(rig.refetches(), 1);
    let next = cold.run(&pinned)?;
    assert_eq!(next.code, Some(0), "{}\n{}", next.stdout, next.stderr);
    assert!(next.has_line(RAN), "{}", next.stdout);
    assert_eq!(rig.refetches(), 1);
    assert_eq!(cold.trusted_refs()?, "");
    Ok(())
}

#[test]
fn a_pin_the_repair_cannot_restore_fails_closed_after_one_repair_and_a_pin_without_the_action_fails_at_once()
-> Result<()> {
    let lost_on_the_server = Rig::new()?;
    let pinned = default_branch_commit(&lost_on_the_server)?;
    let cold = Cold::new(&lost_on_the_server)?;
    warm(&lost_on_the_server, &cold, &pinned)?;
    lose_loose_blob(&lost_on_the_server.mirror(), &pinned)?;
    lose_loose_blob(&upstream(&lost_on_the_server), &pinned)?;
    let ran = cold.run(&pinned)?;
    assert_fails_naming(&ran, &pinned);
    assert_eq!(lost_on_the_server.refetches(), 1);
    assert_eq!(cold.trusted_refs()?, "");

    let without_the_action = Rig::new()?;
    default_branch_commit(&without_the_action)?;
    let cold = Cold::new(&without_the_action)?;
    let early = without_the_action.head.clone();
    let ran = cold.run(&early)?;
    assert_fails_naming(&ran, &early);
    assert!(
        ran.lines()
            .iter()
            .any(|line| line.contains(&format!("the pin {early} has no {ACTION}"))),
        "{}",
        ran.stdout
    );
    assert!(!ran.stdout.contains("after one repair"), "{}", ran.stdout);
    assert_eq!(without_the_action.refetches(), 0);
    assert_eq!(cold.trusted_refs()?, "");
    Ok(())
}
