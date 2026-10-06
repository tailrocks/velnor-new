//! Rust backend option cache identity and injection regressions.

use std::collections::BTreeMap;

use velnor_actions_workflow_renderer::cache_p08::{infer_job_tools, mise_cache_key_for_tools};

use super::impl_renderer_fixtures::job;

const SPEC: &str = "rust[profile=minimal,components=clippy,rustfmt]@1.98.1";

#[test]
fn minimal_rust_options_are_inferred_and_change_cache_identity() {
    let step = velnor_actions_workflow_renderer::ambient_shell_step(
        "Install Rust",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("mise install '{SPEC}'"),
        ],
        BTreeMap::new(),
    )
    .expect("step");
    let (_, job) = job("demo", "Demo", Vec::new(), vec![step]);
    assert_eq!(infer_job_tools(&job), vec![SPEC.to_owned()]);
    let key = |spec: &str| {
        mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.18", &[spec.to_owned()])
    };
    assert_ne!(
        key(SPEC).expect("typed Rust"),
        key("rust@1.98.1").expect("plain Rust")
    );
    for spec in [
        "rust[profile=default,components=clippy,rustfmt]@1.98.1",
        "rust[profile=minimal,components=clippy,rustfmt,postinstall=evil]@1.98.1",
        "rust[profile=minimal,components=clippy,rustfmt]@stable",
        "rust[profile=minimal,components=clippy,rustfmt]@1..1",
    ] {
        assert!(key(spec).is_err(), "unsupported backend options: {spec}");
    }
}

#[test]
fn desktop_options_bind_exact_compiler_host_and_nested_wrapper() {
    let desktop = "rust[profile=minimal,components=clippy,rustfmt,targets=aarch64-apple-darwin,mr_boxington=true]@1.99.1";
    let key = |spec: &str| {
        mise_cache_key_for_tools("aarch64-apple-darwin", "2026.10.0", &[spec.to_owned()])
    };
    let current = key(desktop).expect("current desktop Rust pin");
    let prior = key(&desktop.replace("1.99.1", "1.98.1")).expect("prior desktop Rust pin");
    assert_ne!(current, prior);
    assert!(key(&desktop.replace("mr_boxington=true", "mr_boxington=false")).is_err());
    assert!(key(&desktop.replace("aarch64-apple-darwin", "x86_64-apple-darwin")).is_err());
}
