// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_mise::cache_sources::{
    ByteCounter, ImmutableSourceObject, SourceByteLedger, SourceHydration, decide_hydration,
    digest_body_hex, is_approved_origin, require_approved_origins, sources_cache_paths,
    validate_sources_subset, verify_restored_object,
};
use velnor_actions_mise::cache_transport::{
    SourceTransfer, SourceTransferDirection, SourceTransferOutcome, apply_source_transfer,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn registry_object() -> ImmutableSourceObject {
    ImmutableSourceObject {
        object_id: "registry-index".to_owned(),
        kind: "source".to_owned(),
        approved_origin_id: "approved-registry".to_owned(),
        content_digest: format!("s256-{ABC_SHA256}"),
    }
}

fn git_object() -> ImmutableSourceObject {
    ImmutableSourceObject {
        object_id: "git-db".to_owned(),
        kind: "source".to_owned(),
        approved_origin_id: "approved-git".to_owned(),
        content_digest: format!("s256-{EMPTY_SHA256}"),
    }
}

fn approved() -> [&'static str; 2] {
    ["approved-registry", "approved-git"]
}

fn check_restore_scopes_consumed_home() -> Result<(), Box<dyn Error>> {
    let home = "consumed-cargo-home";
    let paths = sources_cache_paths(home)?;
    if paths.len() != 3 {
        return Err(fail("sources subset drifted"));
    }
    validate_sources_subset(&paths, home)?;
    for bad in [
        format!("{home}/registry/src/index"),
        format!("{home}/../escape"),
        "foreign-home/registry/index".to_owned(),
    ] {
        if validate_sources_subset(std::slice::from_ref(&bad), home).is_ok() {
            return Err(fail(&format!("unscoped restore path admitted: {bad}")));
        }
    }
    Ok(())
}

fn check_warm_needs_no_origin_fetch() -> Result<(), Box<dyn Error>> {
    let required = vec![registry_object(), git_object()];
    require_approved_origins(&required, &approved())?;
    if !verify_restored_object(&required[0], true, b"abc")? {
        return Err(fail("restored registry object rejected"));
    }
    if !verify_restored_object(&required[1], true, b"")? {
        return Err(fail("restored git object rejected"));
    }
    let present = vec!["registry-index".to_owned(), "git-db".to_owned()];
    if decide_hydration(&required, &present)? != SourceHydration::Warm {
        return Err(fail("covered sources not warm"));
    }
    let mut ledger = SourceByteLedger::new(
        ByteCounter::unknown("origin_acquisition_not_needed"),
        ByteCounter::measured(854),
    );
    ledger.validate()?;
    if ledger.record_origin(1).is_ok() {
        return Err(fail("unknown origin bytes admitted a write"));
    }
    if ledger.origin_bytes.value.is_some() {
        return Err(fail("unknown origin bytes zero-filled"));
    }
    ledger.record_cache(100)?;
    if ledger.cache_bytes.value != Some(954) {
        return Err(fail("cache transfer bytes misaccounted"));
    }
    if digest_body_hex(&required[0].content_digest)? != ABC_SHA256 {
        return Err(fail("digest body drifted"));
    }
    Ok(())
}

fn check_cold_fetches_only_missing() -> Result<(), Box<dyn Error>> {
    let required = vec![registry_object(), git_object()];
    if !verify_restored_object(&required[0], true, b"abc")? {
        return Err(fail("warm object missed"));
    }
    if verify_restored_object(&required[1], false, b"")? {
        return Err(fail("unrestored bytes trusted"));
    }
    let present = vec!["registry-index".to_owned()];
    match decide_hydration(&required, &present)? {
        SourceHydration::Warm => Err(fail("missing object reported warm")),
        SourceHydration::ColdFetch { missing_object_ids } => {
            if missing_object_ids != vec!["git-db".to_owned()] {
                return Err(fail("fetch set covers present objects"));
            }
            Ok(())
        }
    }
}

fn check_transfers_keep_origins_and_cache_separate() -> Result<(), Box<dyn Error>> {
    let mut ledger = SourceByteLedger::new(ByteCounter::measured(10), ByteCounter::measured(854));
    let origin = SourceTransfer {
        direction: SourceTransferDirection::OriginDownload,
        object_id: "git-db".to_owned(),
        transferred_bytes: 5,
        outcome: SourceTransferOutcome::Completed,
    };
    apply_source_transfer(&mut ledger, &origin)?;
    if ledger.origin_bytes.value != Some(15) || ledger.cache_bytes.value != Some(854) {
        return Err(fail("origin download touched cache bytes"));
    }
    let cached = SourceTransfer {
        direction: SourceTransferDirection::CacheTransfer,
        object_id: "registry-index".to_owned(),
        transferred_bytes: 46,
        outcome: SourceTransferOutcome::Completed,
    };
    apply_source_transfer(&mut ledger, &cached)?;
    if ledger.origin_bytes.value != Some(15) || ledger.cache_bytes.value != Some(900) {
        return Err(fail("cache transfer touched origin bytes"));
    }
    let failed = SourceTransfer {
        direction: SourceTransferDirection::OriginDownload,
        object_id: "git-db".to_owned(),
        transferred_bytes: 7,
        outcome: SourceTransferOutcome::Failed,
    };
    apply_source_transfer(&mut ledger, &failed)?;
    if ledger.origin_bytes.value != Some(15) || ledger.cache_bytes.value != Some(900) {
        return Err(fail("failed transfer recorded bytes"));
    }
    Ok(())
}

#[test]
fn gc1_t11_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_restore_scopes_consumed_home()?;
    check_warm_needs_no_origin_fetch()?;
    check_cold_fetches_only_missing()?;
    check_transfers_keep_origins_and_cache_separate()?;
    Ok(())
}

fn check_rejects_bad_identities() -> Result<(), Box<dyn Error>> {
    let mut bad_digest = registry_object();
    ABC_SHA256.clone_into(&mut bad_digest.content_digest);
    if bad_digest.validate().is_ok() {
        return Err(fail("digest without prefix admitted"));
    }
    let mut upper = registry_object();
    upper.content_digest = format!("s256-{}", ABC_SHA256.to_uppercase());
    if upper.validate().is_ok() {
        return Err(fail("uppercase digest admitted"));
    }
    let mut short = registry_object();
    "s256-abc".clone_into(&mut short.content_digest);
    if short.validate().is_ok() {
        return Err(fail("short digest admitted"));
    }
    let mut kind = registry_object();
    "tarball".clone_into(&mut kind.kind);
    if kind.validate().is_ok() {
        return Err(fail("unknown object kind admitted"));
    }
    let mut origin = registry_object();
    origin.approved_origin_id = String::new();
    if origin.validate().is_ok() {
        return Err(fail("empty origin admitted"));
    }
    if is_approved_origin("", &approved()) || is_approved_origin("approved-registry", &[]) {
        return Err(fail("origin approval drifted"));
    }
    let required = vec![registry_object(), git_object()];
    if require_approved_origins(&required, &["approved-registry"]).is_ok() {
        return Err(fail("unapproved origin admitted"));
    }
    Ok(())
}

fn check_rejects_untrusted_or_tampered_bytes() -> Result<(), Box<dyn Error>> {
    let object = registry_object();
    if verify_restored_object(&object, true, b"abd")? {
        return Err(fail("tampered bytes verified"));
    }
    if verify_restored_object(&object, false, b"abc")? {
        return Err(fail("unrestored bytes trusted"));
    }
    let duplicated = vec![registry_object(), registry_object()];
    if decide_hydration(&duplicated, &[]).is_ok() {
        return Err(fail("duplicate object admitted"));
    }
    Ok(())
}

fn check_rejects_counter_conflation() -> Result<(), Box<dyn Error>> {
    let both = ByteCounter {
        value: Some(1),
        unknown_reason: Some("gap"),
    };
    if both.validate().is_ok() {
        return Err(fail("measured and unknown conflated"));
    }
    let neither = ByteCounter {
        value: None,
        unknown_reason: None,
    };
    if neither.validate().is_ok() {
        return Err(fail("absent counter admitted"));
    }
    let mut unknown = ByteCounter::unknown("cache_transfer_unobserved");
    if unknown.record(10).is_ok() {
        return Err(fail("unknown counter admitted a write"));
    }
    if unknown.value.is_some() {
        return Err(fail("unknown counter zero-filled"));
    }
    let mut saturated = ByteCounter::measured(u64::MAX);
    if saturated.record(1).is_ok() {
        return Err(fail("byte overflow admitted"));
    }
    let bad_transfer = SourceTransfer {
        direction: SourceTransferDirection::CacheTransfer,
        object_id: String::new(),
        transferred_bytes: 1,
        outcome: SourceTransferOutcome::Completed,
    };
    let mut ledger = SourceByteLedger::new(ByteCounter::measured(0), ByteCounter::measured(0));
    if apply_source_transfer(&mut ledger, &bad_transfer).is_ok() {
        return Err(fail("anonymous transfer applied"));
    }
    if ledger.cache_bytes.value != Some(0) {
        return Err(fail("rejected transfer mutated ledger"));
    }
    Ok(())
}

#[test]
fn gc1_t11_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_identities()?;
    check_rejects_untrusted_or_tampered_bytes()?;
    check_rejects_counter_conflation()?;
    Ok(())
}
