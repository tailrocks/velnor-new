// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_tofu_hydration() -> Result<(), Box<dyn Error>> {
    let valid = format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/providers-1_2");
    if !tofu_providers_path_ok(&valid) {
        return Err(fail("valid provider path rejected"));
    }
    for bad in [
        String::new(),
        "elsewhere".to_owned(),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/"),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/has/slash"),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/has..dots"),
    ] {
        if tofu_providers_path_ok(&bad) {
            return Err(fail(&format!("invalid provider path hydrated: {bad}")));
        }
    }
    Ok(())
}

#[test]
fn gc1_t49_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_stack_routing_table()?;
    check_managed_identities_preserved()?;
    check_plan_payloads()?;
    let bun = bun_plan()?;
    check_keys_shape(&bun, false)?;
    check_keys_shape(&bun, true)?;
    let both = [bun_plan()?, browsers_plan()];
    if prelude(&both)?.len() != 3 || prelude_for(&both, false)?.len() != 3 {
        return Err(fail("family prelude lost its pairing"));
    }
    check_after_producer_shapes()?;
    check_admission_table()?;
    check_inventory_classes()?;
    check_tofu_hydration()?;
    let restore = family_restore_step(&dbt_plan()?)?;
    if restore.role != Some(StepRole::FamilyRestore) {
        return Err(fail("direct restore lost its role"));
    }
    let keys = family_keys_step(&[dbt_plan()?])?;
    if keys.id != Some(StepId::FamilyKeys) {
        return Err(fail("direct keys lost their identity"));
    }
    let size = family_size_step(&venv_plan())?;
    if size.role != Some(StepRole::FamilySizeCheck) {
        return Err(fail("direct size check lost its role"));
    }
    let save = family_save_step(&venv_plan())?;
    if save.role != Some(StepRole::FamilySave) {
        return Err(fail("direct save lost its role"));
    }
    Ok(())
}

fn rejects_managed_and_misplaced_plans() -> Result<(), Box<dyn Error>> {
    let managed = managed_plan();
    if prelude_for(std::slice::from_ref(&managed), false).is_ok()
        || after_producer(&managed).is_ok()
        || family_keys_step(std::slice::from_ref(&managed)).is_ok()
        || family_restore_step(&managed).is_ok()
        || family_size_step(&managed).is_ok()
        || family_save_step(&managed).is_ok()
    {
        return Err(fail("managed family entered the stack path"));
    }
    let shared_with_locator = FamilyCachePlan {
        family: CacheFamily::BunInstall,
        locator: Some("app".to_owned()),
        snapshot: FamilySnapshot::Files(vec!["app/bun.lock".to_owned()]),
        compat: Vec::new(),
    };
    if prelude_for(std::slice::from_ref(&shared_with_locator), false).is_ok()
        || after_producer(&shared_with_locator).is_ok()
        || family_save_step(&shared_with_locator).is_ok()
    {
        return Err(fail("shared payload accepted a locator"));
    }
    let rooted_without_locator = FamilyCachePlan {
        family: CacheFamily::DbtPackages,
        locator: None,
        snapshot: FamilySnapshot::Files(vec!["shop/package-lock.yml".to_owned()]),
        compat: Vec::new(),
    };
    if prelude_for(std::slice::from_ref(&rooted_without_locator), false).is_ok()
        || after_producer(&rooted_without_locator).is_ok()
        || family_save_step(&rooted_without_locator).is_ok()
    {
        return Err(fail("per-root payload lost its locator"));
    }
    Ok(())
}

fn rejects_malformed_payloads() -> Result<(), Box<dyn Error>> {
    let escape = FamilyCachePlan {
        family: CacheFamily::BunInstall,
        locator: None,
        snapshot: FamilySnapshot::Files(vec!["../evil".to_owned()]),
        compat: Vec::new(),
    };
    if prelude_for(std::slice::from_ref(&escape), false).is_ok()
        || after_producer(&escape).is_ok()
        || family_save_step(&escape).is_ok()
    {
        return Err(fail("escaping snapshot validated"));
    }
    let hostile_compat = FamilyCachePlan {
        family: CacheFamily::PythonVenv,
        locator: None,
        snapshot: FamilySnapshot::Files(vec!["requirements.txt".to_owned()]),
        compat: vec!["oops!".to_owned()],
    };
    if family_keys_step(std::slice::from_ref(&hostile_compat)).is_ok()
        || family_save_step(&hostile_compat).is_ok()
    {
        return Err(fail("hostile compat line validated"));
    }
    if family_keys_step(&[]).is_ok() {
        return Err(fail("empty family plan validated"));
    }
    Ok(())
}

#[test]
fn gc1_t49_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    rejects_managed_and_misplaced_plans()?;
    rejects_malformed_payloads()?;
    if admit_family_candidate(false, None)
        != FamilyAdmission::Blocked(FAMILY_BLOCKED_UNKNOWN_INVENTORY)
    {
        return Err(fail("doubly-unknown state escaped blocking"));
    }
    for bad in [
        String::new(),
        "elsewhere".to_owned(),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/"),
    ] {
        if tofu_providers_path_ok(&bad) {
            return Err(fail("invalid provider path hydrated"));
        }
    }
    Ok(())
}
