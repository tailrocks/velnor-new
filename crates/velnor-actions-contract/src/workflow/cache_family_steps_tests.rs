use super::*;

#[test]
fn every_stack_family_has_distinct_output_stems_ids_and_conditions() {
    let mut stems = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    for family in STACK_FAMILIES {
        assert!(is_stack_family(family));
        assert!(stems.insert(family_output_stem(family)));
        assert!(ids.insert(family_size_step_id(family)));
        assert_eq!(family_for_stem(&family_output_stem(family)), Some(family));
        let condition = family_save_condition(family);
        assert!(condition.starts_with("success() && steps.family-keys.outputs.save == 'true'"));
        assert!(condition.contains(&family_size_step_id(family)));
    }
    assert!(!is_stack_family(CacheFamily::Sources));
    assert!(!is_stack_family(CacheFamily::MbxBundle));
}

#[test]
fn key_expressions_name_the_family_stem_and_stay_space_free() {
    let key = family_key_expr(CacheFamily::PythonVenv);
    assert_eq!(key, "${{steps.family-keys.outputs.python_venv_key}}");
    assert_eq!(
        family_restore_keys_expr(CacheFamily::BunInstall),
        "${{steps.family-keys.outputs.bun_install_restore_keys}}"
    );
}

#[test]
fn the_size_step_ids_match_the_typed_step_ids() {
    use crate::workflow::step_id::StepId;
    let typed = [
        StepId::FamilySizeBunInstall,
        StepId::FamilySizePlaywrightBrowsers,
        StepId::FamilySizeDbtPackages,
        StepId::FamilySizePythonVenv,
        StepId::FamilySizeAnsibleCollections,
    ];
    for (family, id) in STACK_FAMILIES.into_iter().zip(typed) {
        assert_eq!(family_size_step_id(family), id.as_str());
    }
}

#[test]
fn a_root_locator_is_one_safe_segment() {
    assert_eq!(root_locator("").as_deref(), Some("root"));
    assert_eq!(root_locator(".").as_deref(), Some("root"));
    assert_eq!(
        root_locator("analytics/dbt").as_deref(),
        Some("analytics-dbt")
    );
    for bad in ["/abs", "a//b", "../x", "a/../b", "a b", "a;b"] {
        assert_eq!(root_locator(bad), None, "{bad}");
    }
}
