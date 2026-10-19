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
        CacheFamily::Rsdeps,
        "708f240869a7cc80d28ed561a3bede05bacdf5aee0ee35146d75e3bb943f5bb5",
    ),
    (
        CacheFamily::RsdepsHelper,
        "f6eb7dbcc6cb70eb6ed64ea98c84c9f890c46e92dae196600819def8ff1ab7c7",
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
fn rsdeps_specs_are_root_prefixed_workspace_profiles() {
    let spec = CacheFamily::Rsdeps.spec();
    assert!(spec.is_per_root() && spec.root_scoped);
    assert_eq!(
        spec.path_input(Some("")),
        "target/debug\ntarget/velnor-clippy/debug\ntarget/velnor-doc/debug"
    );
    assert_eq!(
        CacheFamily::RsdepsHelper.spec().path_input(Some("nested/")),
        "nested/target/release"
    );
    assert_eq!(CacheFamily::Rsdeps.key_prefix(), "velnor-v2-rsdeps-");
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
    assert_eq!(
        CacheFamily::for_key("velnor-v2-rsdeps-helper-trusted-r0-x"),
        Some(CacheFamily::RsdepsHelper)
    );
    assert_eq!(
        CacheFamily::Rsdeps.max_key_bytes(),
        17 + 7 + 1 + 14 + 69 + 1 + 69
    );
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
    assert!(cache_output_reserve_utf16_bytes(2, 0) > cache_output_reserve_utf16_bytes(0, 0));
}
