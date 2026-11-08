// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use velnor_actions_contract::StepKind;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::verifier_builders::{
    ToolchainPhase, TreeDigestPin, rust_verifier_shell_step, verify_rust_owned_argv,
};

use crate::fixture::{
    CHANNEL_URL, FIXTURE_COMPONENTS, FIXTURE_COMPONENTS_SHA256, FIXTURE_TREE_SHA256, Fixture,
    HELPERS, MISMATCH, PHASES, TARGET, VERSION, fixture_pin, script,
};
use crate::{REAL_COMPONENTS, REAL_COMPONENTS_SHA256};

const DECOY_TREE_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub(crate) fn derive_tool() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/derive-rust-tree-digest")
}

#[test]
fn p1_matching_tree_is_accepted_offline_in_both_phases() {
    let fx = Fixture::standard();
    for phase in PHASES {
        fx.run(&script(phase, &[fixture_pin()]))
            .assert_offline_pinned();
    }
}

#[test]
fn readonly_verifiers_share_the_tree_without_replacing_the_manifest() {
    let fx = Fixture::standard();
    let manifest = fx
        .root()
        .join("lib/rustlib/multirust-channel-manifest.toml");
    let original_inode = fs::metadata(&manifest).expect("manifest").ino();
    let before = fx.snapshot();
    let code = script(ToolchainPhase::ReadOnly, &[fixture_pin()]);
    std::thread::scope(|scope| {
        let handles = (0..8)
            .map(|_| {
                let fx = &fx;
                let code = &code;
                scope.spawn(move || {
                    let runner = tempfile::tempdir().expect("runner");
                    let cargo = tempfile::tempdir().expect("cargo");
                    fs::create_dir(cargo.path().join("bin")).expect("cargo bin");
                    fs::write(cargo.path().join("bin/sentinel"), b"present").expect("sentinel");
                    let outcome = fx.run_with_env(
                        code,
                        Path::new("/usr/bin"),
                        &[
                            ("RUNNER_TEMP", runner.path().to_str().expect("runner path")),
                            ("CARGO_HOME", cargo.path().to_str().expect("cargo path")),
                        ],
                    );
                    outcome.assert_offline_pinned();
                    assert!(cargo.path().join("bin/sentinel").is_file());
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().expect("verifier thread");
        }
    });
    assert_eq!(
        fs::metadata(&manifest).expect("manifest").ino(),
        original_inode
    );
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn readonly_verifier_rejects_a_tampered_channel_without_repairing_it() {
    let fx = Fixture::standard();
    fx.put(
        "lib/rustlib/multirust-channel-manifest.toml",
        b"tampered",
        0o644,
    );
    let before = fx.snapshot();
    let outcome = fx.run(&script(ToolchainPhase::ReadOnly, &[fixture_pin()]));
    assert!(
        outcome
            .stderr
            .contains("rust_channel_manifest_digest_mismatch"),
        "{outcome:?}"
    );
    assert!(outcome.requests.is_empty(), "{outcome:?}");
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn p2_derive_tool_and_verifier_agree_on_the_fixture() {
    let fx = Fixture::standard();
    let output = crate::derive::timed(derive_tool())
        .arg("--tree")
        .arg(fx.root())
        .output()
        .expect("run derive tool");
    assert!(output.status.success(), "{output:?}");
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        printed.contains(&format!("components_sha256={FIXTURE_COMPONENTS_SHA256}\n")),
        "{printed}"
    );
    assert!(
        printed.contains(&format!("tree_sha256={FIXTURE_TREE_SHA256}\n")),
        "{printed}"
    );
    for phase in PHASES {
        fx.run(&script(phase, &[fixture_pin()]))
            .assert_offline_pinned();
    }
}

#[test]
fn p3_live_emitters_select_the_production_pin() {
    let catalog = ToolCatalog::pinned();
    assert_eq!(catalog.version(PinnedTool::Rust), VERSION);
    for phase in PHASES {
        let owned = verify_rust_owned_argv(&catalog, phase);
        let step = rust_verifier_shell_step(&catalog, phase).expect("verifier step");
        let StepKind::Shell { run, .. } = &step.kind else {
            panic!("verifier step is not a shell step: {step:?}");
        };
        for script in [owned.last().expect("script"), run.last().expect("script")] {
            let fx = Fixture::with_components(REAL_COMPONENTS);
            let before = fx.snapshot();
            let outcome = fx.run(script);
            outcome.assert_hard_failed(if phase == ToolchainPhase::Restored {
                MISMATCH
            } else {
                "rust_channel_manifest_digest_mismatch"
            });
            if phase == ToolchainPhase::Restored {
                assert!(outcome.stdout.contains("tree_differs"), "{outcome:?}");
                assert!(outcome.requests.is_empty(), "{outcome:?}");
                Fixture::standard().run(script).assert_hard_failed(MISMATCH);
            } else {
                assert_eq!(outcome.requests, [CHANNEL_URL], "{outcome:?}");
            }
            assert_eq!(fx.snapshot(), before);
        }
    }
}

#[test]
fn p6_a_target_with_several_rows_selects_by_the_components_digest() {
    let decoy = TreeDigestPin::new(VERSION, TARGET, REAL_COMPONENTS_SHA256, DECOY_TREE_SHA256);
    let widened = format!("{FIXTURE_COMPONENTS}rust-src-x86_64-unknown-linux-gnu\n");
    for pins in [[decoy, fixture_pin()], [fixture_pin(), decoy]] {
        for phase in PHASES {
            Fixture::standard()
                .run(&script(phase, &pins))
                .assert_pinned();
            let fx = Fixture::standard();
            fx.put("lib/rustlib/components", widened.as_bytes(), 0o644);
            fx.run(&script(phase, &pins)).assert_hard_failed(MISMATCH);
        }
    }
}

#[test]
fn n6_hostile_helpers_on_path_and_in_the_tree_are_never_run() {
    let hostile = tempfile::tempdir().expect("hostile dir");
    for helper in HELPERS {
        let path = hostile.path().join(helper);
        let body = format!(
            "#!/bin/sh\n: > \"$VELNOR_TEST_MARKER\"\nprintf '%s  -\\n' {FIXTURE_TREE_SHA256}\n"
        );
        fs::write(&path, body).expect("write fake");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fake");
    }
    for phase in PHASES {
        let accepted = Fixture::standard();
        accepted
            .run_with_path(&script(phase, &[fixture_pin()]), hostile.path())
            .assert_pinned();
        assert!(!accepted.marker().exists());
        let flipped = Fixture::standard();
        flipped.put("lib/x.so", b"sO\n", 0o644);
        flipped
            .run_with_path(&script(phase, &[fixture_pin()]), hostile.path())
            .assert_hard_failed(MISMATCH);
        assert!(!flipped.marker().exists());
    }
}
