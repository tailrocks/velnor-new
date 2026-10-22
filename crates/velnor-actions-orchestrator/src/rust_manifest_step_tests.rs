//! `Verify Rust channel manifest` step tests.

use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::catalog::{RUST_CHANNEL_MANIFESTS, RUST_VERSION};

use super::{NOT_CATALOGUED_CODE, VERIFY_RUST_MANIFEST_STEP, verify_rust_manifest_step};

fn run_text(catalog: &ToolCatalog) -> (String, std::collections::BTreeMap<String, String>) {
    let step = verify_rust_manifest_step(catalog).expect("step");
    assert_eq!(step.name, VERIFY_RUST_MANIFEST_STEP);
    assert_eq!(step.role, Some(StepRole::VerifyRustManifest));
    let StepKind::Shell { run, env } = step.kind else {
        panic!("shell step");
    };
    (run.join(" "), env)
}

#[test]
fn the_step_carries_every_catalogued_digest_and_fails_closed_otherwise() {
    let (run, _) = run_text(&ToolCatalog::pinned());
    for (version, digest, _) in RUST_CHANNEL_MANIFESTS {
        assert!(
            run.contains(&format!("{version}) want={digest};;")),
            "{version}"
        );
    }
    assert!(run.contains(&format!("*) fail \"{NOT_CATALOGUED_CODE}: ")));
    assert!(run.contains("https://static.rust-lang.org/dist/channel-rust-$v.toml"));
    assert!(run.contains("--proto \"=https\""), "{run}");
    assert!(run.contains("sha256sum"), "{run}");
    assert!(run.contains("shasum -a 256"), "macOS fallback: {run}");
    // File by file against the pinned component tarballs, not rustup's
    // own metadata.
    for needle in [
        "lib/rustlib/components",
        "https://static.rust-lang.org/dist/*.tar.xz",
        "rust_component_tarball_digest_mismatch",
        "tar -xJf",
        "rust_toolchain_bytes_mismatch",
        "rust_toolchain_file_unexpected",
    ] {
        assert!(run.contains(needle), "{needle}: {run}");
    }
    assert!(
        !run.contains("comm -23"),
        "metadata subset checks prove nothing about the bytes: {run}"
    );
    assert!(
        !run.contains("rm -rf \"${CARGO_HOME:?}/bin\""),
        "only the restored phase discards rustup"
    );
}

#[test]
fn the_restored_step_discards_the_cached_rustup_and_tolerates_an_empty_cache() {
    let step = super::verify_restored_rust_step(&ToolCatalog::pinned()).expect("step");
    assert_eq!(step.name, super::VERIFY_RESTORED_RUST_STEP);
    let StepKind::Shell { run, env } = step.kind else {
        panic!("shell step");
    };
    let run = run.join(" ");
    assert!(run.contains("rm -rf \"${CARGO_HOME:?}/bin\""), "{run}");
    assert!(run.contains("no restored Rust"), "{run}");
    assert!(run.contains("rust_toolchain_bytes_mismatch"), "{run}");
    assert_eq!(
        env.get("CARGO_HOME").map(String::as_str),
        Some("${{ runner.temp }}/velnor/cargo")
    );
    assert_eq!(
        env.get("RUSTUP_HOME").map(String::as_str),
        Some("${{ runner.temp }}/velnor/rustup")
    );
}

#[test]
fn the_step_is_credential_free_and_reads_the_owned_rustup_home() {
    let (run, env) = run_text(&ToolCatalog::pinned());
    assert_eq!(
        env.get("RUSTUP_HOME").map(String::as_str),
        Some("${{ runner.temp }}/velnor/rustup")
    );
    assert_eq!(env.get("GITHUB_TOKEN").map(String::as_str), Some(""));
    assert!(run.contains("unset"), "credential unset prelude: {run}");
}

#[test]
fn project_declared_rust_stays_a_run_time_reference() {
    let catalog = ToolCatalog::pinned()
        .with_project_tool(velnor_actions_mise::PinnedTool::Rust, RUST_VERSION)
        .expect("exact")
        .for_emission();
    let (run, _) = run_text(&catalog);
    assert!(run.contains("v=\"${VELNOR_TOOL_RUST}\""), "{run}");
    assert!(
        !run.contains(&format!("v=\"{RUST_VERSION}\"")),
        "a project-declared version is never copied: {run}"
    );
}
