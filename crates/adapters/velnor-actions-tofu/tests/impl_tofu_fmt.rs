//! Formatting-scope (S2) inclusion, exclusion, and per-root sets.
use velnor_actions_tofu_core::fmt_scope::{
    covered_fmt_roots, fmt_scope_for_root, fmt_set, is_excluded_name, is_fmt_file, under_hidden_dir,
};

/// Repo-relative path list from names.
fn paths(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn inclusion_covers_s2_set_only() {
    for name in [
        "main.tf",
        "main.tofu",
        "terraform.tfvars",
        "custom.tfvars",
        "main.tftest.hcl",
        "main.tofutest.hcl",
    ] {
        assert!(is_fmt_file(name), "{name}");
    }
    for name in [
        "main.tf.json",
        "main.tofu.json",
        "custom.tofuvars",
        "values.tfvars.json",
        ".terraform.lock.hcl",
        "README.md",
        "main.tf.example",
        "terraform.tfvars.example",
    ] {
        assert!(!is_fmt_file(name), "{name}");
    }
}

#[test]
fn bare_suffixes_never_format() {
    for name in [".tf", ".tofu", ".tfvars", ".tftest.hcl", ".tofutest.hcl"] {
        assert!(!is_fmt_file(name), "{name}");
    }
}

#[test]
fn exclusion_predicate_marks_dot_tilde_hash() {
    assert!(is_excluded_name(".foo.tf"));
    assert!(is_excluded_name("~draft.tf"));
    assert!(is_excluded_name("#main.tf#"));
    assert!(is_excluded_name("#"));
    assert!(!is_excluded_name("main.tf"));
    assert!(!is_excluded_name("#main.tf"));
    assert!(!is_excluded_name("main.tf#"));
    assert!(!is_excluded_name("a#b.tf"));
}

#[test]
fn excluded_names_never_format() {
    assert!(!is_fmt_file(".foo.tf"));
    assert!(!is_fmt_file("~draft.tofu"));
    assert!(!is_fmt_file("#main.tf#"));
}

#[test]
fn hidden_dirs_excluded_at_any_depth() {
    assert!(under_hidden_dir(".terraform/modules/x/main.tf"));
    assert!(under_hidden_dir("infra/.cache/a.tfvars"));
    assert!(!under_hidden_dir("infra/main.tf"));
    assert!(!under_hidden_dir("main.tf"));
}

#[test]
fn fmt_set_skips_hidden_dirs_and_json() {
    let selected = fmt_set(&paths(&[
        "main.tf",
        ".terraform/modules/x/main.tf",
        "infra/main.tofu.json",
        "infra/vars.tfvars",
    ]));
    assert_eq!(selected, paths(&["infra/vars.tfvars", "main.tf"]));
}

#[test]
fn fmt_set_is_sorted_and_precedence_free() {
    let selected = fmt_set(&paths(&["main.tf", "main.tofu", "extra.tf"]));
    assert_eq!(selected, paths(&["extra.tf", "main.tf", "main.tofu"]));
}

#[test]
fn root_scope_covers_the_repo_recursively() {
    let selected = fmt_scope_for_root(&paths(&["main.tf", "infra/a.tofu", "infra/deep/b.tf"]), "");
    assert_eq!(
        selected,
        paths(&["infra/a.tofu", "infra/deep/b.tf", "main.tf"])
    );
}

#[test]
fn nested_scope_selects_its_subtree_only() {
    let files = paths(&["main.tf", "infra/a.tofu", "infra/deep/b.tf", "other/c.tf"]);
    assert_eq!(
        fmt_scope_for_root(&files, "infra"),
        paths(&["infra/a.tofu", "infra/deep/b.tf"])
    );
}

#[test]
fn scope_prefix_never_matches_siblings() {
    let files = paths(&["infra2/a.tf", "infra/a.tf"]);
    assert_eq!(fmt_scope_for_root(&files, "infra"), paths(&["infra/a.tf"]));
}

#[test]
fn override_and_test_files_format() {
    let selected = fmt_set(&paths(&[
        "a_override.tf",
        "override.tofu",
        "check.tftest.hcl",
        "check.tofutest.hcl",
    ]));
    assert_eq!(selected.len(), 4);
}

#[test]
fn empty_selection_stays_empty() {
    assert!(fmt_set(&[]).is_empty());
    assert!(fmt_scope_for_root(&paths(&["main.tf.json"]), ".").is_empty());
}

/// Normalized root list from names.
fn roots(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn repo_root_covers_every_subdir() {
    let covered = covered_fmt_roots(&roots(&["", "stacks/a"]));
    assert_eq!(covered, roots(&["stacks/a"]).into_iter().collect());
}

#[test]
fn ancestor_covers_nested_but_never_siblings() {
    let covered = covered_fmt_roots(&roots(&["stacks", "stacks/nested", "stacks-sibling"]));
    assert_eq!(covered, roots(&["stacks/nested"]).into_iter().collect());
}

#[test]
fn disjoint_roots_cover_nothing() {
    let covered = covered_fmt_roots(&roots(&["stacks/a", "stacks/b"]));
    assert!(covered.is_empty());
}

#[test]
fn deep_chains_cover_every_descendant() {
    let covered = covered_fmt_roots(&roots(&["a", "a/b", "a/b/c"]));
    assert_eq!(covered, roots(&["a/b", "a/b/c"]).into_iter().collect());
}

/// Family-derived fmt oracle: native configs/overrides, tests, and
/// non-JSON vars format; JSON spellings, the lock, and others never.
fn family_expects_fmt(name: &str) -> bool {
    use velnor_actions_tofu_core::effective::{Dialect, config_shape};
    use velnor_actions_tofu_core::family::{Family, family_of};
    use velnor_actions_tofu_core::fmt_scope::is_excluded_name;
    if is_excluded_name(name) {
        return false;
    }
    match family_of(name) {
        Family::Config | Family::Override => {
            matches!(
                config_shape(name).map(|shape| shape.dialect),
                Some(Dialect::Native)
            )
        }
        Family::Test => true,
        Family::Var => name.strip_suffix(".json").is_none(),
        Family::Lock | Family::Other => false,
    }
}

/// The fmt inclusion list and the family arms enumerate the same S2
/// set: every corpus name agrees with its family derivation.
#[test]
fn fmt_inclusion_matches_family_derivation() {
    for name in [
        "main.tf",
        "main.tofu",
        "main.tf.json",
        "main.tofu.json",
        "override.tf",
        "network_override.tofu",
        "override.tf.json",
        "main.tftest.hcl",
        "main.tofutest.hcl",
        "a.tfvars",
        "a.tfvars.json",
        "terraform.tfvars",
        "terraform.tfvars.json",
        "prod.auto.tfvars",
        "prod.auto.tfvars.json",
        "vars.tofuvars",
        ".terraform.lock.hcl",
        "README.md",
        "main.tf.example",
        ".hidden.tf",
        "~scratch.tofu",
        "#temp.tf#",
        "nested.tftest.hcl.json",
    ] {
        assert_eq!(
            is_fmt_file(name),
            family_expects_fmt(name),
            "{name} drifts between the fmt list and families"
        );
    }
}
