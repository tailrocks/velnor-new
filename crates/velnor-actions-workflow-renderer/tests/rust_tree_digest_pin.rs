// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

#[cfg(test)]
#[path = "rust_tree_digest_pin/fixture.rs"]
mod fixture;

#[cfg(test)]
#[path = "rust_tree_digest_pin/outcome.rs"]
mod outcome;

#[cfg(test)]
#[path = "rust_tree_digest_pin/release.rs"]
mod release;

#[cfg(test)]
#[path = "rust_tree_digest_pin/bookkeeping.rs"]
mod bookkeeping;

#[cfg(test)]
#[path = "rust_tree_digest_pin/pinned.rs"]
mod pinned;

#[cfg(test)]
#[path = "rust_tree_digest_pin/derive.rs"]
mod derive;

#[cfg(test)]
#[path = "rust_tree_digest_pin/derive_cli.rs"]
mod derive_cli;

#[cfg(test)]
#[path = "rust_tree_digest_pin/hard_fail.rs"]
mod hard_fail;

#[cfg(test)]
#[path = "rust_tree_digest_pin/manifest.rs"]
mod manifest;

use fixture::{
    CHANNEL_URL, FIXTURE_COMPONENTS, FIXTURE_COMPONENTS_SHA256, FIXTURE_TREE_SHA256, Fixture,
    MISMATCH, Outcome, PHASES, Row, TARGET, VERSION, fixture_pin, run_rows, script, sha256_hex,
};
use velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS;
use velnor_actions_mise::{RUST_VERSION, ToolCatalog};
use velnor_actions_workflow_renderer::verifier_builders::{
    RUST_TREE_DIGESTS, TreeDigestPin, verify_rust_argv,
};

pub(crate) const REAL_COMPONENTS: &str = "cargo-x86_64-unknown-linux-gnu\nclippy-preview-x86_64-unknown-linux-gnu\nrust-std-x86_64-unknown-linux-gnu\nrustc-x86_64-unknown-linux-gnu\nrustfmt-preview-x86_64-unknown-linux-gnu\n";

pub(crate) const REAL_COMPONENTS_SHA256: &str =
    "e3808a4c78c316b41a42ed88010c6bbf24ae24a348a5a22b8e3bc62328db1981";

#[test]
fn p7_default_home_verifier_never_applies_the_owned_home_pins() {
    let argv = verify_rust_argv(&ToolCatalog::pinned());
    let script = argv.last().expect("script");
    for components in [FIXTURE_COMPONENTS, REAL_COMPONENTS] {
        let fx = Fixture::with_components(components);
        let before = fx.snapshot();
        let outcome = fx.run(script);
        let combined = outcome.combined();
        assert!(
            !combined.contains(MISMATCH)
                && !combined.contains("pin_mismatch")
                && !combined.contains("matches pinned")
                && outcome
                    .stderr
                    .contains("rust_channel_manifest_digest_mismatch")
                && outcome.requests == [CHANNEL_URL],
            "{outcome:?}"
        );
        assert_eq!(fx.snapshot(), before);
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
    assert_eq!(
        sha256_hex(REAL_COMPONENTS.as_bytes()),
        REAL_COMPONENTS_SHA256
    );
    assert_eq!(
        sha256_hex(FIXTURE_COMPONENTS.as_bytes()),
        FIXTURE_COMPONENTS_SHA256
    );
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
    run_rows(&rows, &Outcome::no_pin);
}

#[test]
fn n7_a_missing_pin_reports_the_observed_components_digest() {
    for phase in PHASES {
        let outcome = Fixture::standard().run(&script(phase, &[]));
        outcome.assert_fell_through();
        assert!(
            outcome.stdout.contains(&format!(
                "::notice::pin_mismatch: no_pin components={FIXTURE_COMPONENTS_SHA256}\n"
            )),
            "{outcome:?}"
        );
    }
}
