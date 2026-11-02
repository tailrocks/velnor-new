// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

#[cfg(test)]
#[path = "rust_tree_digest_pin/fixture.rs"]
mod fixture;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use fixture::{
    FIXTURE_COMPONENTS, FIXTURE_COMPONENTS_SHA256, FIXTURE_TREE_SHA256, Fixture, HELPERS, PHASES,
    Row, TARGET, VERSION, fixture_pin, run_rows, script,
};
use sha2::{Digest, Sha256};
use velnor_actions_contract::StepKind;
use velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS;
use velnor_actions_mise::{PinnedTool, RUST_VERSION, ToolCatalog};
use velnor_actions_workflow_renderer::verifier_builders::{
    RUST_TREE_DIGESTS, ToolchainPhase, TreeDigestPin, rust_verifier_shell_step,
    verify_rust_owned_argv,
};

const REAL_COMPONENTS: &str = "cargo-x86_64-unknown-linux-gnu\nclippy-preview-x86_64-unknown-linux-gnu\nrust-std-x86_64-unknown-linux-gnu\nrustc-x86_64-unknown-linux-gnu\nrustfmt-preview-x86_64-unknown-linux-gnu\n";

const REAL_COMPONENTS_SHA256: &str =
    "e3808a4c78c316b41a42ed88010c6bbf24ae24a348a5a22b8e3bc62328db1981";

const UNSAFE_NAMES: [(&str, &str); 4] = [
    (
        "a b",
        "e103c304a93bba11dbfe7cb9608f3b6039ce28ff807aa4ed29759435ad07c3ef",
    ),
    (
        "a$b",
        "afb2a2c8cbf712c2dbf6f4573cbe77ef4fc347fe359e644c16e13a18a92df66a",
    ),
    (
        "a*b",
        "5626c79bf93b91b4a01de84fc520d426bfcc843375ff1921b4fee8384f372a18",
    ),
    (
        "a\nb",
        "0515b57e2fa3843d16787185cfbc0149d5f2c00872c93ef3d64c40b3835ef3a0",
    ),
];

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .concat()
}

fn derive_tool() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/derive-rust-tree-digest")
}

#[test]
fn p1_matching_tree_is_accepted_offline_in_both_phases() {
    for phase in PHASES {
        let fx = Fixture::standard();
        fx.run(&script(phase, &[fixture_pin()])).assert_pinned();
    }
}

#[test]
fn p2_derive_tool_and_verifier_agree_on_the_fixture() {
    let fx = Fixture::standard();
    let output = Command::new(derive_tool())
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
    fx.run(&script(ToolchainPhase::Installed, &[fixture_pin()]))
        .assert_pinned();
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
            let outcome = fx.run(script);
            outcome.assert_fell_through();
            assert!(outcome.stdout.contains("tree_differs"), "{outcome:?}");
        }
    }
}

#[test]
fn u1_every_pin_is_catalogued_and_well_formed() {
    for pin in RUST_TREE_DIGESTS {
        assert!(
            RUST_CHANNEL_MANIFESTS
                .iter()
                .any(|(known, _, _)| *known == pin.version()),
            "{pin:?} is not in RUST_CHANNEL_MANIFESTS"
        );
        for digest in [pin.components_sha256(), pin.tree_sha256()] {
            assert!(
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
                "{pin:?}"
            );
        }
    }
}

#[test]
fn u2_current_rust_has_a_linux_x64_pin() {
    assert!(
        RUST_TREE_DIGESTS
            .iter()
            .any(|pin| pin.version() == RUST_VERSION && pin.target() == TARGET),
        "no RUST_TREE_DIGESTS row for Rust {RUST_VERSION} {TARGET}"
    );
}

#[test]
fn u3_committed_components_literal_matches_the_current_pin() {
    assert_eq!(sha256_hex(REAL_COMPONENTS), REAL_COMPONENTS_SHA256);
    assert_eq!(sha256_hex(FIXTURE_COMPONENTS), FIXTURE_COMPONENTS_SHA256);
    assert!(
        RUST_TREE_DIGESTS
            .iter()
            .any(|pin| pin.version() == RUST_VERSION
                && pin.target() == TARGET
                && pin.components_sha256() == REAL_COMPONENTS_SHA256),
        "no RUST_TREE_DIGESTS row for Rust {RUST_VERSION} {TARGET} with the real components"
    );
}

type Case<'a> = (&'a str, fixture::Mutation<'a>, &'a str);

fn row<'a>(name: &str, mutate: fixture::Mutation<'a>) -> Row<'a> {
    Row {
        name: name.to_owned(),
        mutate,
        pin: fixture_pin(),
    }
}

#[test]
fn n1_any_tree_mutation_falls_through_to_the_full_path() {
    let widened = format!("{FIXTURE_COMPONENTS}rust-src-x86_64-unknown-linux-gnu\n").into_bytes();
    let widen = |fx: &Fixture| fx.put("lib/rustlib/components", &widened, 0o644);
    let rows = [
        row("flipped byte", &|fx| fx.put("lib/x.so", b"sO\n", 0o644)),
        row("extra file", &|fx| fx.put("lib/extra.so", b"e", 0o644)),
        row("extra dot file", &|fx| fx.put("share/.extra", b"e", 0o644)),
        row("missing file", &|fx| fx.remove("lib/x.so")),
        row("empty dir added", &|fx| fx.dir("lib/emptyb", 0o755)),
        row("empty dir removed", &|fx| fx.remove("etc/empty")),
        row("setuid file", &|fx| fx.chmod("bin/rustc", 0o4755)),
        row("setgid dir", &|fx| fx.chmod("share", 0o2755)),
        row("sticky dir", &|fx| fx.chmod("lib", 0o1755)),
        row("world writable file", &|fx| fx.chmod("lib/x.so", 0o666)),
        row("altered components", &widen),
    ];
    run_rows(&rows, fixture::Outcome::fell_through);
}

#[test]
fn n2_unsafe_path_characters_are_never_pinned() {
    let mutations = UNSAFE_NAMES.map(|(name, _)| {
        move |fx: &Fixture| {
            fx.put(&format!("lib/{name}"), b"x", 0o644);
        }
    });
    let rows = UNSAFE_NAMES
        .iter()
        .zip(&mutations)
        .map(|((name, literal), mutate)| Row {
            name: format!("{name:?}"),
            mutate,
            pin: TreeDigestPin::new(VERSION, TARGET, FIXTURE_COMPONENTS_SHA256, literal),
        })
        .collect::<Vec<_>>();
    run_rows(&rows, fixture::Outcome::fell_through);
}

#[test]
fn n3_symlinks_are_refused_before_the_pin() {
    let file_link = |fx: &Fixture| fx.link("lib/l", "x.so");
    let dir_link = |fx: &Fixture| fx.link("lib/ld", "../share");
    let dangling = |fx: &Fixture| fx.link("lib/dang", "/nonexistent");
    let root_link = |fx: &Fixture| fx.symlink_root();
    let rows: [Case<'_>; 4] = [
        ("file symlink", &file_link, "rust_toolchain_file_unexpected"),
        ("dir symlink", &dir_link, "rust_toolchain_file_unexpected"),
        (
            "dangling symlink",
            &dangling,
            "rust_toolchain_file_unexpected",
        ),
        (
            "symlinked toolchain dir",
            &root_link,
            "rust_toolchain_missing",
        ),
    ];
    for (name, mutate, code) in rows {
        for phase in PHASES {
            let fx = Fixture::standard();
            mutate(&fx);
            let outcome = fx.run(&script(phase, &[fixture_pin()]));
            assert!(
                outcome.is_not_pinned() && outcome.stderr.contains(code),
                "{name} {phase:?}: {outcome:?}"
            );
        }
    }
}

#[test]
fn n4_toolchains_must_hold_exactly_one_entry() {
    let rows = [
        row("second same-version dir", &|fx| {
            fx.sibling("1.99.1-aarch64-unknown-linux-gnu");
        }),
        row("different name", &|fx| {
            fx.sibling("nightly-x86_64-unknown-linux-gnu");
        }),
        row("dot entry", &|fx| fx.sibling(".hidden")),
    ];
    run_rows(&rows, fixture::Outcome::is_not_pinned);
}

#[test]
fn n5_a_pin_for_another_version_or_target_is_not_used() {
    let rows = [
        Row {
            name: "other version".to_owned(),
            mutate: &|_| (),
            pin: TreeDigestPin::new(
                "1.99.0",
                TARGET,
                FIXTURE_COMPONENTS_SHA256,
                FIXTURE_TREE_SHA256,
            ),
        },
        Row {
            name: "other target".to_owned(),
            mutate: &|_| (),
            pin: TreeDigestPin::new(
                VERSION,
                "aarch64-unknown-linux-gnu",
                FIXTURE_COMPONENTS_SHA256,
                FIXTURE_TREE_SHA256,
            ),
        },
    ];
    run_rows(&rows, fixture::Outcome::fell_through);
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
            .assert_fell_through();
        assert!(!flipped.marker().exists());
    }
}
