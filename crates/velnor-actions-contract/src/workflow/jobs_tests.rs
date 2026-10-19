//! Crate-job ID and display-name unit tests.
//!
//! Declared via `#[path]` from `jobs.rs` under `cfg(test)`.

use super::{
    TOFU_DISPLAY_PREFIX, ValidatorKind, assign_crate_job_ids, assign_rust_crate_job_ids,
    crate_display_label, crate_display_name, is_safe_display_name, tofu_display_name,
    validate_job_id,
};
use std::collections::BTreeSet;

#[test]
fn display_names_reject_expressions_and_controls() {
    assert!(is_safe_display_name("Rust / demo"));
    assert!(is_safe_display_name("Plan"));
    for bad in [
        "Rust / ${{ secrets.x }}",
        "x\ny",
        "x\ry",
        "x\ty",
        "x\x07y",
        "${{",
    ] {
        assert!(!is_safe_display_name(bad), "{bad:?} must fail");
    }
    assert_eq!(
        crate_display_label("demo", "crates/demo/Cargo.toml"),
        "demo"
    );
    assert_eq!(
        crate_display_label("", "crates/${{x}}/Cargo.toml"),
        "$?{{x}}"
    );
    assert_eq!(crate_display_label("", "crates/a\nb/Cargo.toml"), "a?b");
    assert_eq!(crate_display_name("", "crates/a/Cargo.toml"), "Rust / a");
    assert!(is_safe_display_name(&crate_display_name(
        "",
        "crates/${{x}}/Cargo.toml"
    )));
}

#[test]
fn crate_job_ids_ignore_task_configuration_variants() {
    let packages = BTreeSet::from([
        ("package-a".to_owned(), "velnor-core".to_owned()),
        ("package-b".to_owned(), "velnor-cli".to_owned()),
    ]);
    let ids = assign_rust_crate_job_ids(&packages, "rust-");

    assert_eq!(ids.len(), packages.len());
    assert_eq!(ids["package-a"], "rust-velnor-core");
    assert_eq!(ids["package-b"], "rust-velnor-cli");
}

#[test]
fn crate_job_id_collisions_keep_distinct_package_ids() {
    let packages = BTreeSet::from([
        ("package-a".to_owned(), "same-name".to_owned()),
        ("package-b".to_owned(), "same-name".to_owned()),
    ]);
    let ids = assign_rust_crate_job_ids(&packages, "rust-");

    assert_eq!(ids.len(), 2);
    assert_ne!(ids["package-a"], ids["package-b"]);
}

#[test]
fn configuration_group_ids_keep_tofu_variants() {
    let groups = BTreeSet::from([
        (
            "package-a".to_owned(),
            "infra".to_owned(),
            "default".to_owned(),
        ),
        (
            "package-a".to_owned(),
            "infra".to_owned(),
            "checks".to_owned(),
        ),
    ]);
    let ids = assign_crate_job_ids(&groups, "tofu-");

    assert_eq!(ids.len(), 2);
    assert_eq!(
        ids[&("package-a".to_owned(), "default".to_owned())],
        "tofu-infra"
    );
    assert_eq!(
        ids[&("package-a".to_owned(), "checks".to_owned())],
        "tofu-infra-checks"
    );
}

#[test]
fn tofu_display_names_pin_prefix_and_sanitize() {
    assert_eq!(tofu_display_name("stacks/a"), "OpenToFu — stacks/a");
    assert_eq!(tofu_display_name("."), "OpenToFu — .");
    assert!(
        tofu_display_name("x").starts_with(TOFU_DISPLAY_PREFIX),
        "single prefix source"
    );
    assert!(is_safe_display_name(&tofu_display_name("stacks/a")));
    assert_eq!(tofu_display_name("${{x}}"), "OpenToFu — $?{{x}}");
    assert!(is_safe_display_name(&tofu_display_name("${{x}}")));
    assert_eq!(tofu_display_name("a\nb"), "OpenToFu — a?b");
    assert!(is_safe_display_name(&tofu_display_name("a\nb")));
}

#[test]
fn branding_gate_skips_package_slugs_only() {
    assert!(validate_job_id("plan").is_ok());
    assert!(validate_job_id("rust-demo").is_ok());
    assert!(validate_job_id("rust-velnor-actions-contract").is_ok());
    assert!(validate_job_id("tofu-velnor-infra").is_ok());
    assert!(validate_job_id("velnor-plan").is_err());
    assert!(validate_job_id("task-velnor-final").is_err());
    assert!(validate_job_id("").is_err());
}

#[test]
fn verify_names_resolve_exactly() {
    for kind in ValidatorKind::consumer_verify() {
        assert_eq!(ValidatorKind::from_verify_name(kind.job_id()), Some(kind));
    }
    for name in ["cargo-deny", "cargo-machete", "actionlint", "plan", ""] {
        assert_eq!(ValidatorKind::from_verify_name(name), None, "{name}");
    }
}
