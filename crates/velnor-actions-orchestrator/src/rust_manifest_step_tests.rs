//! `Verify Rust channel manifest` step tests.

use velnor_actions_contract::StepKind;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::catalog::{RUST_CHANNEL_MANIFESTS, RUST_VERSION};

use super::{NOT_CATALOGUED_CODE, VERIFY_RUST_MANIFEST_STEP, verify_rust_manifest_step};

#[path = "rust_manifest_step_tests/execution.rs"]
mod execution;

fn run_text(catalog: &ToolCatalog) -> (String, std::collections::BTreeMap<String, String>) {
    let step = verify_rust_manifest_step(catalog).expect("The Rust manifest step must render.");
    assert_eq!(step.name, VERIFY_RUST_MANIFEST_STEP);
    let StepKind::Shell { run, env } = step.kind else {
        panic!("The step must run a shell command.");
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
    assert!(run.contains(&format!(
        "*) printf \"%s\\n\" \"::error::{NOT_CATALOGUED_CODE}"
    )));
    assert!(run.contains("https://static.rust-lang.org/dist/channel-rust-$v.toml"));
    assert!(run.contains("--proto \"=https\""), "{run}");
    assert!(run.contains("sha256sum -c"), "{run}");
    assert!(
        run.contains("shasum -a 256 -c"),
        "The step must include the macOS digest command: {run}."
    );
    assert!(
        run.contains(&format!(
            "$RUSTUP_HOME/toolchains/{RUST_VERSION}-x86_64-unknown-linux-gnu/lib/rustlib/multirust-channel-manifest.toml"
        )),
        "{run}"
    );
    assert!(
        run.contains("comm -23"),
        "Each recorded installed-manifest hash must appear in the pinned manifest: {run}."
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
    assert!(
        run.contains("unset"),
        "The step must unset credentials before verification: {run}."
    );
}

#[test]
fn project_declared_rust_stays_a_run_time_reference() {
    let catalog = ToolCatalog::pinned()
        .with_project_tool(velnor_actions_mise::PinnedTool::Rust, RUST_VERSION)
        .expect("The tool catalog must resolve.")
        .for_emission();
    let (run, _) = run_text(&catalog);
    assert!(run.contains("v=\"${VELNOR_TOOL_RUST}\""), "{run}");
    assert!(
        !run.contains(&format!("v=\"{RUST_VERSION}\"")),
        "The generated text must not copy the project version: {run}."
    );
}
