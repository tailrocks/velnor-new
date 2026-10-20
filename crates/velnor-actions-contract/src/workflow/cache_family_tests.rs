use super::*;

/// Pinned `(schema, paths)` digests. Editing a family's paths without a
/// schema bump changes the digest and fails here; bump the schema, then
/// update the pin.
const PINS: [(CacheFamily, &str); 4] = [
    (
        CacheFamily::Sources,
        "fc961241ea3626cce35dbd7f53c4634c97edca039bf4b911bdbe1ce545db5ce9",
    ),
    (
        CacheFamily::TofuProviders,
        "c6c86468be7d44a4fcd95ac816c3eb3f633aea470ad28c6b2fd5713a8cd4d6e9",
    ),
    (
        CacheFamily::BunInstall,
        "f5a2d9deebae93f54781889a0499930dfe2bc75484928c62ae156d1b5a598811",
    ),
    (
        CacheFamily::PlaywrightBrowsers,
        "bc3b248d99fb5a69cf51d264ae132f4876a70489ee8c41548ecbb8b9ff926a0d",
    ),
];

#[test]
fn family_specs_are_pinned_to_their_schema() {
    for (family, pin) in PINS {
        assert_eq!(spec_pin_digest(family.spec()), pin, "{family:?}");
    }
}

#[test]
fn path_edit_without_schema_bump_changes_the_pin() {
    let edited = FamilySpec {
        paths: &["${{ runner.temp }}/velnor/cargo/registry/index"],
        ..*CacheFamily::Sources.spec()
    };
    assert_ne!(spec_pin_digest(&edited), PINS[0].1);
}

#[test]
fn sources_spec_is_the_owned_cargo_home_subset() {
    let spec = CacheFamily::Sources.spec();
    let expected: Vec<String> = SOURCE_SUBSET
        .iter()
        .map(|suffix| format!("{CARGO_HOME_EXPR}/{suffix}"))
        .collect();
    assert_eq!(spec.resolved_paths(None), expected);
    assert_eq!(CacheFamily::Sources.key_prefix(), "velnor-v2-sources-");
    assert_eq!(
        spec.shell_paths(None)[0],
        "$RUNNER_TEMP/velnor/cargo/registry/index"
    );
    assert!(!spec.is_per_root());
}

#[test]
fn tofu_spec_is_per_root_under_the_plugin_cache_base() {
    let spec = CacheFamily::TofuProviders.spec();
    assert!(spec.is_per_root());
    assert_eq!(
        spec.path_input(Some("b3-abc")),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/b3-abc")
    );
    assert_eq!(
        CacheFamily::TofuProviders.key_prefix(),
        "velnor-v2-tofu-providers-"
    );
}

#[test]
fn every_family_key_fits_the_github_cap() {
    for family in CacheFamily::ALL {
        family.validate_key_bound().expect("bounded");
        assert_eq!(
            CacheFamily::for_key(&format!("{}trusted-x", family.key_prefix())),
            Some(family)
        );
    }
    assert_eq!(
        CacheFamily::Sources.max_key_bytes(),
        18 + 7 + 1 + 69 + 1 + 69
    );
    assert_eq!(CacheFamily::for_key("velnor-v1-sources-x"), None);
}

#[test]
fn expressions_name_the_typed_step_ids() {
    use super::super::step_identity::StepId;
    for id in [
        StepId::CacheKeys,
        StepId::SourcesRestore,
        StepId::SourcesFetch,
    ] {
        assert!(
            SOURCES_SAVE_CONDITION.contains(&format!("steps.{}.", id.as_str())),
            "{id:?}"
        );
    }
    assert!(SOURCES_KEY_STEP_EXPR.contains(StepId::CacheKeys.as_str()));
    assert!(is_plan_cache_output("cache_trust"));
    assert!(is_plan_cache_output("tofu_providers_key_3"));
    assert!(!is_plan_cache_output("tofu_providers_key_"));
    assert_eq!(
        tofu_providers_key_plan_expr(0),
        "${{needs.plan.outputs.tofu_providers_key_0}}"
    );
    assert!(cache_output_reserve_utf16_bytes(2) > cache_output_reserve_utf16_bytes(0));
}

#[test]
fn bun_families_cache_the_velnor_owned_directories() {
    let bun = CacheFamily::BunInstall.spec();
    assert_eq!(
        bun.resolved_paths(None),
        ["${{ runner.temp }}/velnor/bun-cache"]
    );
    assert_eq!(bun.shell_paths(None), ["$RUNNER_TEMP/velnor/bun-cache"]);
    assert_eq!(
        CacheFamily::BunInstall.key_prefix(),
        "velnor-v1-bun-install-"
    );
    let browsers = CacheFamily::PlaywrightBrowsers.spec();
    assert_eq!(
        browsers.path_input(None),
        "${{ runner.temp }}/velnor/ms-playwright"
    );
    assert_eq!(
        CacheFamily::PlaywrightBrowsers.key_prefix(),
        "velnor-v1-playwright-browsers-"
    );
    assert!(!bun.is_per_root() && !browsers.is_per_root());
    for family in [CacheFamily::BunInstall, CacheFamily::PlaywrightBrowsers] {
        let key = format!("{}trusted-x", family.key_prefix());
        assert_eq!(CacheFamily::for_key(&key), Some(family));
    }
}

/// The 10 GB budget rule: the reference repository's hot families plus the
/// allowance for superseded entries stay within 85 % of the quota.
#[test]
fn reference_family_ceilings_fit_the_budget_rule() {
    const MIB: u64 = 1024 * 1024;
    // ChainArgos reference: four Bun roots, two `OpenTofu` roots, one Playwright
    // version in use. The legacy tools layer measured 1,307,773,873 B and gets
    // 1.5 GiB (cache-measurements.md).
    let tools_allowance = 1536 * MIB;
    let units = |family: CacheFamily| match family {
        CacheFamily::Sources | CacheFamily::PlaywrightBrowsers => 1,
        CacheFamily::TofuProviders => 2,
        CacheFamily::BunInstall => 4,
    };
    let worst: u64 = CacheFamily::ALL
        .into_iter()
        .map(|family| family.spec().worst_case_bytes(units(family)))
        .sum::<u64>()
        + tools_allowance;
    let budget = CACHE_QUOTA_BYTES / 100 * CACHE_BUDGET_PERCENT;
    assert!(worst <= budget, "{worst} > {budget}");
    for family in CacheFamily::ALL {
        let spec = family.spec();
        assert!(
            spec.ceiling_bytes > 0 && spec.retained_entries >= 1,
            "{family:?}"
        );
        assert!(
            spec.ceiling_bytes <= 512 * MIB,
            "{family:?} entry too large"
        );
    }
    // Measured: 30,342,453 B gzip for a 61-package Bun cache (115 MB raw);
    // 328,445,484 B of downloaded Playwright archives for Chromium.
    assert!(CacheFamily::BunInstall.spec().ceiling_bytes > 30_342_453 * 8);
    assert!(CacheFamily::PlaywrightBrowsers.spec().ceiling_bytes > 328_445_484);
}
