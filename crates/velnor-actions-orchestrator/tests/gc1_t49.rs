// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/stack_family_cache.rs"]
mod stack_family_cache;

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use velnor_actions_contract::config::AnsibleCollection;
use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, FAMILY_BLOCKED_EXCEEDS_HARD_CEILING, FAMILY_BLOCKED_UNKNOWN_BOUND,
    FAMILY_BLOCKED_UNKNOWN_INVENTORY, FamilyAdmission, GC1_HARD_CEILING_BYTES,
    GC1_MAX_GENERATION_SLOTS, InventoryKeyClass, TOFU_PROVIDER_CACHE_BASE_EXPR,
    admit_family_candidate, classify_inventory_key, family_contract_policy,
};
use velnor_actions_contract::workflow::cache_family_steps::{
    STACK_FAMILIES, family_key_expr, family_output_stem, family_restore_keys_expr,
    family_save_condition, validate_stack_family_plan,
};
use velnor_actions_contract::workflow::cache_identity::Gc1Family;
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};
use velnor_actions_workflow_renderer::steps::{
    FAMILY_KEYS_NAME, FamilyCachePlan, FamilySnapshot, TOOLS_RESTORE_ACTION_USES, TOOLS_SAVE_USES,
    family_keys_step, family_restore_step, family_save_step, family_size_step,
};
use velnor_actions_workflow_renderer::tofu_cache::tofu_providers_path_ok;

use self::stack_family_cache::{
    after_producer, ansible_collections, bun_install, dbt_packages, playwright_browsers, prelude,
    prelude_for, python_venv,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn locator(plan: &FamilyCachePlan) -> Option<&str> {
    plan.locator.as_deref()
}

fn bun_plan() -> Result<FamilyCachePlan, Box<dyn Error>> {
    Ok(bun_install("app")?)
}

fn dbt_plan() -> Result<FamilyCachePlan, Box<dyn Error>> {
    Ok(dbt_packages("shop")?)
}

fn venv_plan() -> FamilyCachePlan {
    python_venv("requirements/dev.txt")
}

fn browsers_plan() -> FamilyCachePlan {
    playwright_browsers()
}

fn collections_plan() -> FamilyCachePlan {
    ansible_collections(&[
        AnsibleCollection {
            name: "community.general".to_owned(),
            version: "9.0.0".to_owned(),
            sha256: "a".repeat(64),
        },
        AnsibleCollection {
            name: "ansible.posix".to_owned(),
            version: "1.6.0".to_owned(),
            sha256: "b".repeat(64),
        },
    ])
}

fn managed_plan() -> FamilyCachePlan {
    FamilyCachePlan {
        family: CacheFamily::Sources,
        locator: None,
        snapshot: FamilySnapshot::Files(vec!["Cargo.lock".to_owned()]),
        compat: Vec::new(),
    }
}

fn shell_script(step: &Step) -> Result<&str, Box<dyn Error>> {
    match &step.kind {
        StepKind::Shell { run, .. } => run
            .get(2)
            .map(String::as_str)
            .ok_or_else(|| fail("shell step left its inline script position")),
        _ => Err(fail("expected a shell step")),
    }
}

fn action_uses(step: &Step) -> Result<&str, Box<dyn Error>> {
    match &step.kind {
        StepKind::Action { uses, .. } => Ok(uses.as_str()),
        _ => Err(fail("expected an action step")),
    }
}

fn action_with(step: &Step) -> Result<&BTreeMap<String, String>, Box<dyn Error>> {
    match &step.kind {
        StepKind::Action { with, .. } => Ok(with),
        _ => Err(fail("expected an action step")),
    }
}

fn check_stack_routing_table() -> Result<(), Box<dyn Error>> {
    if STACK_FAMILIES.len() != 5 {
        return Err(fail("stack family catalog drifted"));
    }
    for family in STACK_FAMILIES
        .into_iter()
        .chain(std::iter::once(CacheFamily::TofuProviders))
    {
        if family.gc1_family().is_some() {
            return Err(fail(
                "existing family claimed a managed generation identity",
            ));
        }
        let policy = family_contract_policy(family);
        if policy.hard_ceiling_bytes != GC1_HARD_CEILING_BYTES
            || policy.max_generation_slots != GC1_MAX_GENERATION_SLOTS
        {
            return Err(fail("existing family left the common budget policy"));
        }
        family
            .spec()
            .check_subordinate_to_common_ceiling()
            .map_err(|error| fail(&format!("family hint escaped the ceiling: {error}")))?;
        family
            .validate_key_bound()
            .map_err(|error| fail(&format!("family key escaped the cap: {error}")))?;
    }
    for family in STACK_FAMILIES {
        let plan_locator = family.spec().is_per_root().then_some("root");
        validate_stack_family_plan(family, plan_locator)
            .map_err(|error| fail(&format!("supported family missed admission: {error}")))?;
    }
    if validate_stack_family_plan(CacheFamily::TofuProviders, Some("root")).is_ok() {
        return Err(fail("provider family entered the stack path"));
    }
    Ok(())
}

fn check_managed_identities_preserved() -> Result<(), Box<dyn Error>> {
    if CacheFamily::Sources.gc1_family() != Some(Gc1Family::Sources) {
        return Err(fail("source family lost its contract identity"));
    }
    if CacheFamily::MbxBundle.gc1_family() != Some(Gc1Family::State) {
        return Err(fail("complete-state family lost its contract identity"));
    }
    if !CacheFamily::Sources
        .spec()
        .paths
        .iter()
        .any(|path| path.contains("registry/index"))
    {
        return Err(fail("source payload roots drifted"));
    }
    if validate_stack_family_plan(CacheFamily::Sources, None).is_ok()
        || validate_stack_family_plan(CacheFamily::MbxBundle, None).is_ok()
    {
        return Err(fail("managed family entered the stack path"));
    }
    Ok(())
}

fn check_plan_payloads() -> Result<(), Box<dyn Error>> {
    let bun = bun_plan()?;
    if bun.family != CacheFamily::BunInstall
        || locator(&bun).is_some()
        || bun.snapshot != FamilySnapshot::Files(vec!["app/bun.lock".to_owned()])
    {
        return Err(fail("bun family lost its payload"));
    }
    let browsers = browsers_plan();
    if browsers.family != CacheFamily::PlaywrightBrowsers
        || locator(&browsers).is_some()
        || browsers.snapshot != FamilySnapshot::Files(vec![".velnor/playwright.lock".to_owned()])
    {
        return Err(fail("browser family lost its payload"));
    }
    let dbt = dbt_plan()?;
    if dbt.family != CacheFamily::DbtPackages
        || locator(&dbt) != Some("shop")
        || dbt.snapshot != FamilySnapshot::Files(vec!["shop/package-lock.yml".to_owned()])
    {
        return Err(fail("dbt family lost its payload"));
    }
    let venv = venv_plan();
    if venv.family != CacheFamily::PythonVenv
        || venv.snapshot != FamilySnapshot::Files(vec!["requirements/dev.txt".to_owned()])
    {
        return Err(fail("venv family lost its payload"));
    }
    let collections = collections_plan();
    if collections.family != CacheFamily::AnsibleCollections {
        return Err(fail("collections family lost its payload"));
    }
    match &collections.snapshot {
        FamilySnapshot::Lines(lines) => {
            let mut sorted = lines.clone();
            sorted.sort();
            if lines != &sorted || lines.len() != 2 {
                return Err(fail("collections family lost its snapshot order"));
            }
        }
        FamilySnapshot::Files(_) => return Err(fail("collections family changed its snapshot")),
    }
    Ok(())
}

fn check_keys_shape(plan: &FamilyCachePlan, single_writer: bool) -> Result<(), Box<dyn Error>> {
    let steps = prelude_for(std::slice::from_ref(plan), single_writer)?;
    if steps.len() != 2 {
        return Err(fail("family prelude lost its restore pairing"));
    }
    let keys = &steps[0];
    if keys.name != FAMILY_KEYS_NAME
        || keys.id != Some(StepId::FamilyKeys)
        || keys.role != Some(StepRole::FamilyKeys)
    {
        return Err(fail("family keys step lost its identity"));
    }
    let script = shell_script(keys)?;
    for marker in ["sha256sum", "GITHUB_OUTPUT", "trusted"] {
        if !script.contains(marker) {
            return Err(fail(&format!(
                "keys step left the trust discipline: {marker}"
            )));
        }
    }
    if single_writer != script.contains("VELNOR_FAMILY_WRITER") {
        return Err(fail("writer serialization gate drifted"));
    }
    let restore = &steps[1];
    if restore.role != Some(StepRole::FamilyRestore) || restore.condition.is_some() {
        return Err(fail("family restore lost its hydration semantics"));
    }
    if action_uses(restore)? != TOOLS_RESTORE_ACTION_USES {
        return Err(fail("family restore left its pinned action"));
    }
    let with = action_with(restore)?;
    if with.get("key").map(String::as_str) != Some(family_key_expr(plan.family).as_str())
        || with.get("restore-keys").map(String::as_str)
            != Some(family_restore_keys_expr(plan.family).as_str())
    {
        return Err(fail("family restore left its key routing"));
    }
    Ok(())
}

fn check_after_producer_shapes() -> Result<(), Box<dyn Error>> {
    for plan in [
        bun_plan()?,
        browsers_plan(),
        dbt_plan()?,
        venv_plan(),
        collections_plan(),
    ] {
        let output_stem = family_output_stem(plan.family);
        let steps = after_producer(&plan)?;
        if steps.len() != 2 {
            return Err(fail("family producer lost its size and save pairing"));
        }
        let size = &steps[0];
        if size.role != Some(StepRole::FamilySizeCheck) {
            return Err(fail("family size step lost its role"));
        }
        let script = shell_script(size)?;
        for marker in [
            "admitted=true",
            "refuse over_ceiling",
            "refuse size_unreadable",
            "refuse missing_directory",
            "unit=1",
            "unit=1024",
            &format!("{output_stem}_bytes="),
            &format!("{output_stem}_bytes_unit="),
        ] {
            if !script.contains(marker) {
                return Err(fail(&format!(
                    "size step left common bound reporting: {marker}"
                )));
            }
        }
        let save = &steps[1];
        if save.role != Some(StepRole::FamilySave) {
            return Err(fail("family save lost its role"));
        }
        if save.condition.as_deref() != Some(family_save_condition(plan.family).as_str()) {
            return Err(fail("family save bypassed the repository policy"));
        }
        if action_uses(save)? != TOOLS_SAVE_USES {
            return Err(fail("family save left its pinned action"));
        }
        let with = action_with(save)?;
        if with.len() != 2
            || with.get("key").map(String::as_str) != Some(family_key_expr(plan.family).as_str())
        {
            return Err(fail("family save left its bound payload"));
        }
    }
    Ok(())
}

fn check_admission_table() -> Result<(), Box<dyn Error>> {
    for family in STACK_FAMILIES
        .into_iter()
        .chain(std::iter::once(CacheFamily::TofuProviders))
    {
        let output_stem = family_output_stem(family);
        if admit_family_candidate(true, Some(1_000)) != FamilyAdmission::Admitted {
            return Err(fail(&format!(
                "fitting {output_stem} candidate missed admission"
            )));
        }
        if admit_family_candidate(true, None)
            != FamilyAdmission::Blocked(FAMILY_BLOCKED_UNKNOWN_BOUND)
        {
            return Err(fail(&format!(
                "unbounded {output_stem} publication validated as zero"
            )));
        }
        if admit_family_candidate(false, Some(1_000))
            != FamilyAdmission::Blocked(FAMILY_BLOCKED_UNKNOWN_INVENTORY)
        {
            return Err(fail(&format!(
                "unknown {output_stem} inventory validated as complete"
            )));
        }
        if admit_family_candidate(true, Some(GC1_HARD_CEILING_BYTES + 1))
            != FamilyAdmission::Blocked(FAMILY_BLOCKED_EXCEEDS_HARD_CEILING)
        {
            return Err(fail(&format!(
                "oversized {output_stem} candidate escaped the ceiling"
            )));
        }
    }
    Ok(())
}

fn check_inventory_classes() -> Result<(), Box<dyn Error>> {
    if classify_inventory_key("velnor-v2-sources-trusted-deadbeef-r1")
        != InventoryKeyClass::Managed(CacheFamily::Sources)
    {
        return Err(fail("managed source key left its family"));
    }
    if classify_inventory_key("velnor-v1-bun-install-trusted-deadbeef-r1")
        != InventoryKeyClass::Managed(CacheFamily::BunInstall)
    {
        return Err(fail("managed stack key left its family"));
    }
    if classify_inventory_key("velnor-v2-tofu-providers-trusted-deadbeef-r0")
        != InventoryKeyClass::Managed(CacheFamily::TofuProviders)
    {
        return Err(fail("managed provider key left its family"));
    }
    for key in [
        "velnor-gc1-image-linux-amd64-deadbeef",
        "foreign-cache-key",
        "ghaimage-cache-12345",
    ] {
        if classify_inventory_key(key) != InventoryKeyClass::Unmanaged {
            return Err(fail("foreign namespace entered managed families"));
        }
    }
    Ok(())
}

#[path = "gc1_t49_a.rs"]
mod gc1_t49_a;
