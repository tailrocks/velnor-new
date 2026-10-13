//! File-family classification cases.
use velnor_actions_tofu_core::family::{Family, family_of, is_auto_var, is_override_stem};

#[test]
fn config_spellings_classify_config() {
    for name in ["main.tf", "main.tofu", "main.tf.json", "main.tofu.json"] {
        assert_eq!(family_of(name), Family::Config, "{name}");
    }
}

#[test]
fn override_names_classify_override() {
    for name in [
        "override.tf",
        "override.tofu",
        "override.tf.json",
        "override.tofu.json",
        "a_override.tf",
        "z_override.tofu",
        "prod_override.tf.json",
    ] {
        assert_eq!(family_of(name), Family::Override, "{name}");
    }
}

#[test]
fn override_stem_needs_underscore_boundary() {
    assert!(is_override_stem("override"));
    assert!(is_override_stem("a_override"));
    assert!(!is_override_stem("myoverride"));
    assert!(!is_override_stem("override2"));
    assert!(!is_override_stem("main"));
    assert_eq!(family_of("myoverride.tf"), Family::Config);
}

#[test]
fn test_spellings_classify_test() {
    assert_eq!(family_of("main.tftest.hcl"), Family::Test);
    assert_eq!(family_of("main.tofutest.hcl"), Family::Test);
    assert_eq!(family_of(".tftest.hcl"), Family::Other);
}

#[test]
fn var_spellings_classify_var() {
    for name in [
        "terraform.tfvars",
        "extra.auto.tfvars",
        "custom.tfvars",
        "values.tfvars.json",
    ] {
        assert_eq!(family_of(name), Family::Var, "{name}");
    }
}

#[test]
fn tofuvars_is_not_a_var_spelling() {
    assert_eq!(family_of("custom.tofuvars"), Family::Other);
    assert_eq!(family_of("terraform.tofuvars"), Family::Other);
}

#[test]
fn lockfile_classifies_lock() {
    assert_eq!(family_of(".terraform.lock.hcl"), Family::Lock);
    assert_eq!(family_of("terraform.lock.hcl"), Family::Other);
}

#[test]
fn example_and_bare_suffixes_classify_other() {
    for name in [
        "terraform.tfvars.example",
        "main.tf.example",
        ".tf",
        ".tofu",
        "README.md",
        "main.yaml",
    ] {
        assert_eq!(family_of(name), Family::Other, "{name}");
    }
}

#[test]
fn auto_var_names_match() {
    for name in [
        "terraform.tfvars",
        "terraform.tfvars.json",
        "extra.auto.tfvars",
        "extra.auto.tfvars.json",
    ] {
        assert!(is_auto_var(name), "{name}");
    }
}

#[test]
fn plain_var_names_are_not_auto() {
    for name in [
        "custom.tfvars",
        "values.tfvars.json",
        ".auto.tfvars",
        "main.tf",
    ] {
        assert!(!is_auto_var(name), "{name}");
    }
}

#[test]
fn override_json_counts_but_formats_never() {
    assert_eq!(family_of("a_override.tf.json"), Family::Override);
    assert_eq!(family_of("notes.tfvars"), Family::Var);
    assert_eq!(family_of("notes.tfvars.json"), Family::Var);
}

#[test]
fn hidden_looking_names_still_classify_by_suffix() {
    assert_eq!(family_of(".hidden.tf"), Family::Config);
    assert_eq!(family_of("~draft.tofu"), Family::Config);
}
