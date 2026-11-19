// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;

use serde_json::Value;
use velnor_ci::preview::json::digest;
use velnor_ci::preview::runtime::{Host, verify_runtime};

use crate::preview_fakes::{FakeAttestor, FakeNet};
use crate::preview_world::{
    AUTHORITY, PARENT, PUBLIC, PUBLIC_RUN, SOURCE, World, darwin_payload, linux_payload, lock_blob,
};

use crate::preview_mutations::MUTATIONS;

fn attestor(world: &World) -> FakeAttestor {
    FakeAttestor {
        output: world.attestation.clone(),
        seen: RefCell::new(Vec::new()),
    }
}

fn run(world: World, host: Host) -> Result<velnor_ci::preview::runtime::Verified, String> {
    let attestor = attestor(&world);
    let net = FakeNet::new(world);
    verify_runtime(&net, &attestor, host, PUBLIC, PUBLIC_RUN, 1).map_err(|e| e.to_string())
}

#[test]
fn a_valid_world_installs_the_host_payload_and_outputs_both_digests() {
    let verified = run(World::new(), Host::Linux).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(verified.release.linux, linux_payload());
    assert_eq!(verified.release.darwin, darwin_payload());
    assert_eq!(
        verified.outputs(),
        format!(
            "linux_sha256={}\ndarwin_sha256={}\n",
            digest(&linux_payload()),
            digest(&darwin_payload())
        )
    );
    assert_eq!(verified.authority.sha, AUTHORITY);
}

#[test]
fn the_identity_names_the_host_payload_and_the_pinned_authority() {
    let verified = run(World::new(), Host::Darwin).unwrap_or_else(|e| unreachable!("{e}"));
    let identity: Value =
        serde_json::from_slice(&verified.identity()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(identity["target"], "aarch64-apple-darwin");
    assert_eq!(identity["asset_sha256"], digest(&darwin_payload()));
    assert_eq!(identity["source_commit"], SOURCE);
    assert_eq!(identity["authority"]["authority_sha"], AUTHORITY);
    assert_eq!(identity["authority"]["channel"], "source-preview");
}

#[test]
fn every_broken_check_is_refused() {
    for (name, mutate) in MUTATIONS {
        let mut world = World::new();
        mutate(&mut world);
        assert!(run(world, Host::Linux).is_err(), "accepted: {name}");
    }
}

#[test]
fn the_attestation_is_checked_against_the_pinned_authority_and_the_signer_run() {
    let world = World::new();
    let attestor = attestor(&world);
    let net = FakeNet::new(world);
    verify_runtime(&net, &attestor, Host::Linux, PUBLIC, PUBLIC_RUN, 1)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let seen = attestor.seen.borrow();
    assert_eq!(seen.len(), 1);
    let (subject, authority) = &seen[0];
    assert_eq!(authority.sha, AUTHORITY);
    assert_eq!(
        (subject.signer_run_id, subject.signer_run_attempt),
        (500, 3)
    );
    assert_eq!(subject.source_sha, SOURCE);
}

#[test]
fn nothing_is_installed_when_the_preview_is_not_approved() {
    let mut world = World::new();
    world.api.remove(&World::path(&format!(
        "/contents/.velnor/generator.lock?ref={PARENT}"
    )));
    assert!(run(world, Host::Linux).is_err());
}

#[test]
fn a_lock_without_a_runtime_preview_is_not_a_runtime_verification() {
    let mut world = World::new();
    world.api.insert(
        World::path(&format!("/contents/.velnor/generator.lock?ref={PUBLIC}")),
        lock_blob("[other]\nx = 1\n"),
    );
    world.api.insert(
        World::path(&format!("/contents/.velnor/generator.lock?ref={PARENT}")),
        lock_blob("[other]\nx = 1\n"),
    );
    assert!(run(world, Host::Linux).is_err());
}
