//! Effective-set and precedence cases (S1 minimal).
use velnor_actions_tofu::effective::{
    Dialect, config_shape, dir_files, dir_has_effective_config, effective_set,
};

/// Repo-relative path list from names.
fn paths(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn native_shapes_split_by_spelling() {
    let shape = config_shape("main.tf").expect("native config");
    assert_eq!(shape.stem, "main");
    assert_eq!(shape.dialect, Dialect::Native);
    assert!(!shape.tofu_spelling);
    let shape = config_shape("main.tofu").expect("tofu config");
    assert_eq!(shape.stem, "main");
    assert_eq!(shape.dialect, Dialect::Native);
    assert!(shape.tofu_spelling);
}

#[test]
fn json_shapes_split_by_spelling() {
    let shape = config_shape("main.tf.json").expect("json config");
    assert_eq!(shape.stem, "main");
    assert_eq!(shape.dialect, Dialect::Json);
    assert!(!shape.tofu_spelling);
    let shape = config_shape("main.tofu.json").expect("tofu json");
    assert_eq!(shape.stem, "main");
    assert_eq!(shape.dialect, Dialect::Json);
    assert!(shape.tofu_spelling);
}

#[test]
fn longest_suffix_wins_over_prefix() {
    let shape = config_shape("a.tf.json").expect("json, not native");
    assert_eq!(shape.dialect, Dialect::Json);
    assert_eq!(shape.stem, "a");
    let shape = config_shape("override.tofu").expect("override counts");
    assert_eq!(shape.stem, "override");
    assert_eq!(shape.dialect, Dialect::Native);
}

#[test]
fn bare_suffixes_and_non_config_rejected() {
    for name in [".tf", ".tofu", ".tf.json", ".tofu.json"] {
        assert_eq!(config_shape(name), None, "{name}");
    }
    for name in [
        "main.tftest.hcl",
        "main.tofutest.hcl",
        "terraform.tfvars",
        "extra.auto.tfvars",
        "main.yaml",
        "README.md",
        "main.tfvars.json",
    ] {
        assert_eq!(config_shape(name), None, "{name}");
    }
}

#[test]
fn tofu_shadows_tf_same_basename() {
    let survivors = effective_set(&paths(&["main.tf", "main.tofu", "extra.tf"]));
    assert_eq!(survivors, paths(&["extra.tf", "main.tofu"]));
}

#[test]
fn tofu_json_shadows_tf_json() {
    let survivors = effective_set(&paths(&["v.tf.json", "v.tofu.json"]));
    assert_eq!(survivors, paths(&["v.tofu.json"]));
}

#[test]
fn native_and_json_never_suppress_each_other() {
    let survivors = effective_set(&paths(&["main.tf", "main.tf.json"]));
    assert_eq!(survivors, paths(&["main.tf", "main.tf.json"]));
    let survivors = effective_set(&paths(&["main.tofu", "main.tf.json"]));
    assert_eq!(survivors, paths(&["main.tf.json", "main.tofu"]));
}

#[test]
fn precedence_groups_stay_per_directory() {
    let survivors = effective_set(&paths(&["main.tofu", "child/main.tf"]));
    assert_eq!(survivors, paths(&["child/main.tf", "main.tofu"]));
}

#[test]
fn survivors_come_out_sorted() {
    let survivors = effective_set(&paths(&["zeta.tf", "alpha.tf", "mid/beta.tf"]));
    assert_eq!(survivors, paths(&["alpha.tf", "mid/beta.tf", "zeta.tf"]));
}

#[test]
fn dir_files_lists_direct_children_only() {
    let files = paths(&["main.tf", "child/main.tf", "child/nested/x.tf"]);
    assert_eq!(dir_files(&files, ""), paths(&["main.tf"]));
    assert_eq!(dir_files(&files, "child"), paths(&["child/main.tf"]));
    assert_eq!(dir_files(&files, "missing"), [] as [String; 0]);
}

#[test]
fn dir_config_test_ignores_nested_and_shadowed() {
    let files = paths(&["main.tf", "main.tofu", "child/only.tf"]);
    assert!(dir_has_effective_config(&files, ""));
    assert!(dir_has_effective_config(&files, "child"));
    assert!(!dir_has_effective_config(&files, "missing"));
    let files = paths(&["child/only.tf"]);
    assert!(!dir_has_effective_config(&files, ""));
}

#[test]
fn dir_config_test_ignores_test_and_var_files() {
    let files = paths(&["check.tftest.hcl", "terraform.tfvars"]);
    assert!(!dir_has_effective_config(&files, ""));
}
