//! Crate-job ID and display-name unit tests.
//!
//! Canonical sibling suite for `super` (declared `#[cfg(test)] mod tests;`).

use super::{
    TOFU_DISPLAY_PREFIX, crate_display_label, crate_display_name, is_safe_display_name,
    tofu_display_name, validate_job_id,
};

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
    assert_eq!(
        crate_display_name("", "crates/a/Cargo.toml", "default"),
        "Rust / a"
    );
    assert!(is_safe_display_name(&crate_display_name(
        "",
        "crates/${{x}}/Cargo.toml",
        "evil\ncfg"
    )));
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
