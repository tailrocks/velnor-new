// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::fixture::{PHASES, fixture_pin};
use crate::release::{Bookkeeping, Release, TARBALL_URL, fixture_with};

#[test]
fn b1_rustup_rewritten_manifest_of_the_verified_release_passes_the_full_path() {
    for phase in PHASES {
        let (fx, release) = fixture_with(&Release::installed);
        let outcome = release.run(&fx, phase);
        assert!(
            outcome.code == Some(0) && outcome.stdout.contains("verified file by file"),
            "{phase:?}: {outcome:?}"
        );
        let decoy = release.run_pinned(&fx, phase, &[fixture_pin()]);
        assert!(
            decoy.code == Some(0)
                && decoy.stdout.contains("verified file by file")
                && decoy.stdout.contains("pin_mismatch: no_pin")
                && !decoy.stdout.contains("matches pinned"),
            "{phase:?}: {decoy:?}"
        );
    }
}

#[test]
fn b2_a_rewritten_bookkeeping_manifest_fails_the_full_path_by_name() {
    let evil_url = |release: &Release| {
        release
            .installed()
            .replace(TARBALL_URL, "https://attacker.example/evil.tar.xz")
    };
    let evil_hash = |release: &Release| {
        release
            .installed()
            .replace(&release.tarball_sha256, &"0".repeat(64))
    };
    let evil_extra_package = |release: &Release| {
        format!(
            "{}\n[pkg.clippy-preview.target.x86_64-unknown-linux-gnu]\nxz_url = \"https://attacker.example/evil-clippy.tar.xz\"\nxz_hash = \"{}\"\n",
            release.installed(),
            "0".repeat(64)
        )
    };
    let evil_unspaced = |release: &Release| {
        format!(
            "{}\n[pkg.clippy-preview.target.x86_64-unknown-linux-gnu]\nxz_url=\"https://attacker.example/evil-clippy.tar.xz\"\n",
            release.installed()
        )
    };
    let evil_multiline = |release: &Release| {
        format!(
            "{}\n[pkg.clippy-preview.target.x86_64-unknown-linux-gnu]\njunk = \"\"\"\n[pkg.cargo.target.x86_64-unknown-linux-gnu]\n\"\"\"\n",
            release.installed()
        )
    };
    let cases: [(&str, Bookkeeping<'_>, &str); 5] = [
        ("other url", &evil_url, "rust_bookkeeping_manifest_mismatch"),
        (
            "other hash",
            &evil_hash,
            "rust_bookkeeping_manifest_mismatch",
        ),
        (
            "added package",
            &evil_extra_package,
            "rust_bookkeeping_manifest_mismatch",
        ),
        (
            "unspaced assignment",
            &evil_unspaced,
            "rust_bookkeeping_manifest_unreadable",
        ),
        (
            "multi-line string",
            &evil_multiline,
            "rust_bookkeeping_manifest_unreadable",
        ),
    ];
    for (name, bookkeeping, code) in cases {
        for phase in PHASES {
            let (fx, release) = fixture_with(bookkeeping);
            let outcome = release.run(&fx, phase);
            assert!(
                outcome.is_not_pinned()
                    && outcome.stderr.contains(code)
                    && !outcome.stdout.contains("verified file by file"),
                "{name} {phase:?}: {outcome:?}"
            );
        }
    }
}

#[test]
fn b3_a_missing_bookkeeping_manifest_fails_the_full_path() {
    for phase in PHASES {
        let (fx, release) = fixture_with(&Release::installed);
        fx.remove("lib/rustlib/multirust-channel-manifest.toml");
        let outcome = release.run(&fx, phase);
        assert!(
            outcome.is_not_pinned()
                && outcome
                    .stderr
                    .contains("rust_bookkeeping_manifest_unreadable"),
            "{phase:?}: {outcome:?}"
        );
    }
}
