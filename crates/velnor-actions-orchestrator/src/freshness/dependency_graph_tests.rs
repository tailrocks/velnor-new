use std::path::Path;

use serde_json::{Value, json};

use super::*;

fn context() -> FreshnessContext {
    FreshnessContext::new(Path::new("."), false, false)
}

fn package(name: &str, version: &str, source: Option<&str>, dependencies: &[&str]) -> Value {
    let mut package = json!({"name": name, "version": version, "dependencies": dependencies});
    if let Some(source) = source {
        package["source"] = json!(source);
    }
    package
}

fn declaration() -> Declaration {
    Declaration {
        subject: "app:dependencies:serde_json".to_owned(),
        real: "serde_json".to_owned(),
        requirement: TomlValue::String("=1.0.100".to_owned()),
    }
}

#[test]
fn exact_registry_pin_and_complete_graph_pass() {
    let locked = vec![
        package(
            "app",
            "0.1.0",
            None,
            &["serde_json 1.0.100 (registry+https://github.com/rust-lang/crates.io-index)"],
        ),
        package("serde_json", "1.0.100", Some(CRATES_IO), &[]),
    ];
    let members = BTreeSet::from(["app".to_owned()]);
    let mut ctx = context();
    check(&mut ctx, &locked, &[declaration()], &members);
    assert!(ctx.failures.is_empty(), "{:?}", ctx.failures);
}

#[test]
fn stranded_and_dangling_lock_entries_fail() {
    let locked = vec![
        package("app", "0.1.0", None, &["missing 1.0.0"]),
        package("orphan", "9.9.9", Some(CRATES_IO), &[]),
    ];
    let members = BTreeSet::from(["app".to_owned()]);
    let mut ctx = context();
    check(&mut ctx, &locked, &[], &members);
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("dangling edge app -> missing"))
    );
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("unreachable locked package orphan"))
    );
}

#[test]
fn present_non_array_dependency_field_fails_closed() {
    let mut app = package("app", "0.1.0", None, &[]);
    app["dependencies"] = json!({"serde_json": "1.0.100"});
    let locked = vec![app];
    let members = BTreeSet::from(["app".to_owned()]);
    let mut ctx = context();
    check(&mut ctx, &locked, &[], &members);
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("dependencies must be an array"))
    );
}

#[test]
fn non_string_edge_fails_even_when_a_valid_edge_is_present() {
    let mut app = package(
        "app",
        "0.1.0",
        None,
        &["serde_json 1.0.100 (registry+https://github.com/rust-lang/crates.io-index)"],
    );
    app["dependencies"] = json!([
        "serde_json 1.0.100 (registry+https://github.com/rust-lang/crates.io-index)",
        7
    ]);
    let locked = vec![app, package("serde_json", "1.0.100", Some(CRATES_IO), &[])];
    let members = BTreeSet::from(["app".to_owned()]);
    let mut ctx = context();
    check(&mut ctx, &locked, &[], &members);
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("dependency edge 1 must be a string"))
    );
}

#[test]
fn malformed_identity_fields_do_not_default_to_empty_or_local() {
    let mut package = package("third_party", "1.0.0", Some(CRATES_IO), &[]);
    package["version"] = json!(1);
    package["source"] = json!(false);
    let mut ctx = context();
    check(&mut ctx, &[package], &[], &BTreeSet::new());
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("version must be a non-empty string"))
    );
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("source must be a non-empty string"))
    );
}

#[test]
fn missing_dependency_field_remains_a_valid_empty_list() {
    let mut app = package("app", "0.1.0", None, &[]);
    let removed = app
        .as_object_mut()
        .and_then(|object| object.remove("dependencies"));
    assert!(removed.is_some());
    let mut ctx = context();
    check(&mut ctx, &[app], &[], &BTreeSet::from(["app".to_owned()]));
    assert!(ctx.failures.is_empty(), "{:?}", ctx.failures);
}
