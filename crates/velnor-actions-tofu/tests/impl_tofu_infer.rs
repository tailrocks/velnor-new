//! Advisory root inference with child-module exclusion.
use std::collections::BTreeMap;
use velnor_actions_tofu::evidence::classify_with_contents;

/// Index file list from names.
fn files(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// Flattened mise.toml values from dotted keys.
fn mise(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

/// Bounded file contents from entries.
fn contents(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect()
}

#[test]
fn module_target_dirs_excluded_from_inference() {
    let evidence = classify_with_contents(
        &files(&["infra/main.tf", "infra/child/main.tf"]),
        &contents(&[(
            "infra/main.tf",
            "module \"child\" {\n  source = \"./child\"\n}\n",
        )]),
        &mise(&[]),
    );
    assert_eq!(evidence.inferred, files(&["infra"]));
    let evidence = classify_with_contents(
        &files(&["main.tf", "only/main.tf"]),
        &contents(&[("main.tf", "module \"only\" {\n  source = \"./only\"\n}\n")]),
        &mise(&[]),
    );
    assert_eq!(evidence.inferred, files(&["."]));
}

#[test]
fn malformed_content_abandons_inference() {
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[("main.tf", "((( garbage")]),
        &mise(&[]),
    );
    assert_eq!(evidence.inferred, [] as [String; 0]);
}

#[test]
fn remote_and_dynamic_sources_do_not_exclude() {
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[(
            "main.tf",
            "module \"r\" {\n  source = \"ns/name/sys\"\n}\nmodule \"d\" {\n  source = var.x\n}\n",
        )]),
        &mise(&[]),
    );
    assert_eq!(evidence.inferred, files(&["."]));
}
