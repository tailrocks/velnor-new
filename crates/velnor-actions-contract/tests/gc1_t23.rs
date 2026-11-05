// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, Ceiling, FAMILY_BLOCKED_EXCEEDS_HARD_CEILING, FAMILY_BLOCKED_UNKNOWN_BOUND,
    FAMILY_BLOCKED_UNKNOWN_INVENTORY, FamilyAdmission, FamilySpec, GC1_HARD_CEILING_BYTES,
    GC1_MAX_GENERATION_SLOTS, InventoryKeyClass, admit_family_candidate, classify_inventory_key,
    family_contract_policy, outer_entry_bound, outer_tar_bytes, zstd_compress_bound,
};
use velnor_actions_contract::workflow::cache_family_steps::validate_stack_family_plan;
use velnor_actions_contract::workflow::cache_identity::Gc1Family;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn check_decimal_ceiling() -> Result<(), Box<dyn Error>> {
    if GC1_HARD_CEILING_BYTES != 10_000_000_000 {
        return Err(fail("decimal accounting ceiling drifted"));
    }
    if GC1_HARD_CEILING_BYTES == 10 * 1024 * 1024 * 1024 {
        return Err(fail("binary quota constant returned"));
    }
    if GC1_MAX_GENERATION_SLOTS != 2 {
        return Err(fail("generation slot policy drifted"));
    }
    Ok(())
}

fn check_complete_state_binding() -> Result<(), Box<dyn Error>> {
    let spec = CacheFamily::MbxBundle.spec();
    if spec.layer != "state" {
        return Err(fail("complete-state family lost its layer"));
    }
    if spec.paths != [velnor_actions_contract::workflow::cache_family::COMPLETE_STATE_ROOT_EXPR] {
        return Err(fail("complete-state family lost its payload roots"));
    }
    for path in spec.paths {
        if path.to_lowercase().contains("mbx-bundle") {
            return Err(fail("persisted MBX path survived"));
        }
    }
    if CacheFamily::MbxBundle.key_prefix() != "velnor-v5-state-" {
        return Err(fail("complete-state key prefix drifted"));
    }
    if CacheFamily::MbxBundle.gc1_family() != Some(Gc1Family::State) {
        return Err(fail("complete-state family lost its contract identity"));
    }
    if CacheFamily::for_key("velnor-v5-state-trusted-aabb-r0") != Some(CacheFamily::MbxBundle) {
        return Err(fail("complete-state discovery drifted"));
    }
    for retired in [
        "velnor-v1-state-trusted-aabb-r0",
        "velnor-v2-state-trusted-aabb-r0",
        "velnor-v3-state-trusted-aabb-r0",
        "velnor-v4-state-trusted-aabb-r0",
    ] {
        if CacheFamily::for_key(retired).is_some() {
            return Err(fail("complete-state discovery claimed a retired schema"));
        }
    }
    Ok(())
}

fn check_two_bound_ceiling() -> Result<(), Box<dyn Error>> {
    const GIB: u64 = 1024 * 1024 * 1024;
    let spec = |ceiling| FamilySpec {
        layer: "state",
        schema: 4,
        paths: &[],
        ceiling,
        retained_entries: 1,
    };
    let two_bound = Ceiling::StoredAndExtracted {
        stored: GIB * 3 / 2,
        extracted: 12 * GIB,
    };
    if two_bound.extracted() <= GC1_HARD_CEILING_BYTES {
        return Err(fail(
            "fixture must exceed the hard ceiling on the extracted side",
        ));
    }
    spec(two_bound).check_subordinate_to_common_ceiling()?;
    let over_stored = Ceiling::StoredAndExtracted {
        stored: GC1_HARD_CEILING_BYTES + 1,
        extracted: 12 * GIB,
    };
    if spec(over_stored)
        .check_subordinate_to_common_ceiling()
        .is_ok()
    {
        return Err(fail("a stored bound over the hard ceiling was admitted"));
    }
    Ok(())
}

fn check_shipped_state_ceiling() -> Result<(), Box<dyn Error>> {
    let ceiling = CacheFamily::MbxBundle.spec().ceiling;
    if ceiling
        != (Ceiling::StoredAndExtracted {
            stored: 1_610_612_736,
            extracted: 12_884_901_888,
        })
    {
        return Err(fail(
            "complete-state ceiling drifted from 1.5 GiB and 12 GiB",
        ));
    }
    if ceiling.stored() > GC1_HARD_CEILING_BYTES || ceiling.extracted() <= GC1_HARD_CEILING_BYTES {
        return Err(fail("quota accounting must see the stored bound only"));
    }
    Ok(())
}

fn check_outer_entry_bound() -> Result<(), Box<dyn Error>> {
    let stored = CacheFamily::MbxBundle.spec().ceiling.stored();
    if outer_tar_bytes(stored) != Some(1_610_629_120) {
        return Err(fail("outer tar framing drifted at 1.5 GiB"));
    }
    if outer_entry_bound(stored) != Some(1_616_920_640) {
        return Err(fail("outer entry bound drifted at 1.5 GiB"));
    }
    if outer_entry_bound(stored + 10_240) <= outer_entry_bound(stored) {
        return Err(fail(
            "outer entry bound is not increasing with the inner size",
        ));
    }
    if zstd_compress_bound(0) != Some(64) || zstd_compress_bound(131_072) != Some(131_584) {
        return Err(fail("zstd compress bound left the documented formula"));
    }
    if outer_entry_bound(u64::MAX).is_some() {
        return Err(fail("an unbounded inner size was given a bound"));
    }
    Ok(())
}

fn check_retained_distinctions() -> Result<(), Box<dyn Error>> {
    let sources = CacheFamily::Sources.spec();
    if sources.layer != "sources" {
        return Err(fail("source payload distinction drifted"));
    }
    if !sources
        .paths
        .iter()
        .any(|path| path.contains("registry/index"))
    {
        return Err(fail("source payload roots drifted"));
    }
    if CacheFamily::Sources.gc1_family() != Some(Gc1Family::Sources) {
        return Err(fail("source contract identity drifted"));
    }
    let tofu = CacheFamily::TofuProviders.spec();
    if tofu.layer != "tofu-providers" || CacheFamily::TofuProviders.gc1_family().is_some() {
        return Err(fail("tool payload distinction drifted"));
    }
    for family in [
        CacheFamily::BunInstall,
        CacheFamily::PlaywrightBrowsers,
        CacheFamily::DbtPackages,
        CacheFamily::PythonVenv,
        CacheFamily::AnsibleCollections,
    ] {
        if family.gc1_family().is_some() {
            return Err(fail("non-Rust family claimed a Rust identity"));
        }
        let policy = family_contract_policy(family);
        if policy.hard_ceiling_bytes != GC1_HARD_CEILING_BYTES
            || policy.max_generation_slots != GC1_MAX_GENERATION_SLOTS
        {
            return Err(fail("non-Rust family left the common ceiling"));
        }
    }
    for family in CacheFamily::ALL {
        family.validate_key_bound()?;
        family.spec().check_subordinate_to_common_ceiling()?;
        if family_contract_policy(family).hard_ceiling_bytes != GC1_HARD_CEILING_BYTES {
            return Err(fail("family left the common ceiling"));
        }
    }
    Ok(())
}

fn check_common_admission_outcome() -> Result<(), Box<dyn Error>> {
    if admit_family_candidate(true, Some(1_000)) != FamilyAdmission::Admitted {
        return Err(fail("fitting candidate missed admission"));
    }
    if admit_family_candidate(true, Some(GC1_HARD_CEILING_BYTES)) != FamilyAdmission::Admitted {
        return Err(fail("ceiling-sized candidate missed admission"));
    }
    if admit_family_candidate(true, None) != FamilyAdmission::Blocked(FAMILY_BLOCKED_UNKNOWN_BOUND)
    {
        return Err(fail("unknown bound validated as zero"));
    }
    if admit_family_candidate(false, Some(1_000))
        != FamilyAdmission::Blocked(FAMILY_BLOCKED_UNKNOWN_INVENTORY)
    {
        return Err(fail("unknown inventory validated as complete"));
    }
    if admit_family_candidate(true, Some(GC1_HARD_CEILING_BYTES + 1))
        != FamilyAdmission::Blocked(FAMILY_BLOCKED_EXCEEDS_HARD_CEILING)
    {
        return Err(fail("oversized candidate escaped the hard ceiling"));
    }
    Ok(())
}

fn check_stack_binding() -> Result<(), Box<dyn Error>> {
    let policy = validate_stack_family_plan(CacheFamily::BunInstall, None)?;
    if policy.gc1.is_some()
        || policy.hard_ceiling_bytes != GC1_HARD_CEILING_BYTES
        || policy.max_generation_slots != GC1_MAX_GENERATION_SLOTS
    {
        return Err(fail("stack plan lost the common policy"));
    }
    let rooted = validate_stack_family_plan(CacheFamily::DbtPackages, Some("root"))?;
    if rooted.gc1.is_some() {
        return Err(fail("rooted stack plan claimed a Rust identity"));
    }
    Ok(())
}

fn check_inventory_images_stay_unmanaged() -> Result<(), Box<dyn Error>> {
    for key in [
        "velnor-gc1-image-linux-amd64-abcdef",
        "foreign-cache-key",
        "gha-buildkit-linux-amd64-12345",
    ] {
        if classify_inventory_key(key) != InventoryKeyClass::Unmanaged {
            return Err(fail("foreign namespace entered managed families"));
        }
    }
    if classify_inventory_key("velnor-v2-sources-trusted-aabb-r0")
        != InventoryKeyClass::Managed(CacheFamily::Sources)
    {
        return Err(fail("managed source key left its family"));
    }
    Ok(())
}

#[test]
fn gc1_t23_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_decimal_ceiling()?;
    check_complete_state_binding()?;
    check_retained_distinctions()?;
    check_two_bound_ceiling()?;
    check_shipped_state_ceiling()?;
    check_outer_entry_bound()?;
    check_common_admission_outcome()?;
    check_stack_binding()?;
    check_inventory_images_stay_unmanaged()?;
    Ok(())
}

fn rejects_managed_families_on_stack_path() -> Result<(), Box<dyn Error>> {
    for family in [CacheFamily::Sources, CacheFamily::MbxBundle] {
        if validate_stack_family_plan(family, None).is_ok() {
            return Err(fail("managed family entered the stack path"));
        }
    }
    if Gc1Family::parse("mbx").is_ok() {
        return Err(fail("unknown family validated"));
    }
    Ok(())
}

fn rejects_locator_mismatch() -> Result<(), Box<dyn Error>> {
    if validate_stack_family_plan(CacheFamily::BunInstall, Some("root")).is_ok() {
        return Err(fail("shared payload accepted a locator"));
    }
    if validate_stack_family_plan(CacheFamily::DbtPackages, None).is_ok() {
        return Err(fail("per-root payload lost its locator"));
    }
    if validate_stack_family_plan(CacheFamily::DbtPackages, Some("")).is_ok() {
        return Err(fail("empty locator validated"));
    }
    if validate_stack_family_plan(CacheFamily::DbtPackages, Some("../escape")).is_ok() {
        return Err(fail("escaping locator validated"));
    }
    Ok(())
}

#[test]
fn gc1_t23_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    rejects_managed_families_on_stack_path()?;
    rejects_locator_mismatch()?;
    if admit_family_candidate(false, None)
        != FamilyAdmission::Blocked(FAMILY_BLOCKED_UNKNOWN_INVENTORY)
    {
        return Err(fail("doubly-unknown state escaped blocking"));
    }
    Ok(())
}
