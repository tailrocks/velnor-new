// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;

use velnor_actions_workflow_renderer::verifier_builders::TreeDigestPin;

use velnor_actions_workflow_renderer::verifier_builders::{
    ToolchainPhase, TreeCheck, toolchain_script,
};

use crate::fixture::{
    CHANNEL_MANIFEST, CHANNEL_URL, FIXTURE_CHANNEL, FIXTURE_COMPONENTS, FIXTURE_COMPONENTS_SHA256,
    FIXTURE_TREE_SHA256, Fixture, MANIFEST_CARGO, MISMATCH, Mutation, Outcome, PHASES, TARBALL_URL,
    VERSION, fixture_pin, script, script_with, sha256_hex,
};
use crate::release::{Bookkeeping, COMPONENT, Release, fixture_with};

fn evil_rename(release: &Release) -> String {
    format!(
        "{}\n[renames.rustc]\nto = \"rustfmt-preview\"\n",
        release.installed()
    )
}

fn assert_bound(fx: &Fixture, release: &Release) {
    assert_eq!(fx.read(CHANNEL_MANIFEST), release.channel());
    assert_eq!(fx.mode(CHANNEL_MANIFEST), 0o644);
}

#[test]
fn b1_rustup_rewritten_manifest_of_the_verified_release_passes_and_is_bound() {
    for phase in PHASES {
        let (fx, release) = fixture_with(&Release::installed);
        let outcome = release.run(&fx, phase);
        assert!(
            outcome.code == Some(0) && outcome.stdout.contains("verified file by file"),
            "{phase:?}: {outcome:?}"
        );
        assert_bound(&fx, &release);
        let other_target = TreeDigestPin::new(
            VERSION,
            "aarch64-unknown-linux-gnu",
            FIXTURE_COMPONENTS_SHA256,
            FIXTURE_TREE_SHA256,
        );
        let decoy = release.run_pinned(&fx, phase, &[other_target]);
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
fn b6_a_file_by_file_script_verifies_the_release_without_any_pin() {
    for phase in PHASES {
        let (fx, release) = fixture_with(&Release::installed);
        let outcome = release.run_file_by_file(&fx, phase);
        assert!(
            outcome.code == Some(0)
                && outcome.stdout.contains("verified file by file")
                && !outcome.combined().contains("pin_mismatch")
                && !outcome.combined().contains(MISMATCH),
            "{phase:?}: {outcome:?}"
        );
        assert_bound(&fx, &release);
        fx.put("lib/x.so", b"sO\n", 0o644);
        let flipped = release.run_file_by_file(&fx, phase);
        assert!(
            flipped.code == Some(1)
                && flipped.stderr.contains("rust_toolchain_bytes_mismatch")
                && !flipped.stdout.contains("verified file by file"),
            "{phase:?}: {flipped:?}"
        );
    }
}

#[test]
fn b2_any_installed_manifest_is_replaced_by_the_verified_channel_in_the_fallback() {
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
    let garbage = |_: &Release| "\u{0}\u{1}[[[ = = \"\"\"\n\u{7f}".to_owned();
    let empty = |_: &Release| String::new();
    let cases: [(&str, Bookkeeping<'_>); 8] = [
        ("other url", &evil_url),
        ("other hash", &evil_hash),
        ("added package", &evil_extra_package),
        ("unspaced assignment", &evil_unspaced),
        ("multi-line string", &evil_multiline),
        ("added rename", &evil_rename),
        ("binary garbage", &garbage),
        ("empty", &empty),
    ];
    for (name, bookkeeping) in cases {
        for phase in PHASES {
            let (fx, release) = fixture_with(bookkeeping);
            let outcome = release.run(&fx, phase);
            assert!(
                outcome.code == Some(0) && outcome.stdout.contains("verified file by file"),
                "{name} {phase:?}: {outcome:?}"
            );
            assert_bound(&fx, &release);
        }
    }
}

#[test]
fn b3_a_missing_manifest_is_recreated_by_the_fallback() {
    for phase in PHASES {
        let (fx, release) = fixture_with(&Release::installed);
        fx.remove(CHANNEL_MANIFEST);
        let outcome = release.run(&fx, phase);
        assert!(
            outcome.code == Some(0) && outcome.stdout.contains("verified file by file"),
            "{phase:?}: {outcome:?}"
        );
        assert_bound(&fx, &release);
    }
}

#[test]
fn b4_a_pinned_target_never_falls_back_to_the_mode_blind_full_path() {
    let tamper = |fx: &Fixture| {
        fx.put(
            MANIFEST_CARGO,
            b"file:lib/x.so\nfile:../../../victim\n",
            0o644,
        );
        fx.chmod("bin/rustc", 0o4777);
    };
    let unpinned_components = |_: &Fixture| ();
    let extra_toolchain = |fx: &Fixture| fx.sibling("zz");
    let rows: [(&str, Mutation<'_>); 2] = [
        ("unpinned components", &unpinned_components),
        ("extra toolchains entry", &extra_toolchain),
    ];
    for phase in PHASES {
        let (control, release) = fixture_with(&Release::installed);
        let accepted = release.run(&control, phase);
        assert!(
            accepted.code == Some(0) && accepted.stdout.contains("verified file by file"),
            "control {phase:?}: {accepted:?}"
        );
        for (name, select) in rows {
            let (fx, release) = fixture_with(&Release::installed);
            tamper(&fx);
            select(&fx);
            let outcome = release.run_pinned(&fx, phase, &[fixture_pin()]);
            assert!(
                outcome.hard_failed(MISMATCH),
                "{name} {phase:?}: {outcome:?}"
            );
        }
    }
}

#[test]
fn b5_the_full_path_hashes_files_below_a_manifest_named_directory() {
    let rows = [
        ("lib/rustlib/manifest-x", "lib/rustlib/manifest-x/payload"),
        (
            "lib/rustlib/manifest-x/deep",
            "lib/rustlib/manifest-x/deep/payload",
        ),
    ];
    for (dir, file) in rows {
        for phase in PHASES {
            let (fx, release) = fixture_with(&Release::installed);
            fx.dir(dir, 0o755);
            fx.put(file, b"evil\n", 0o644);
            let outcome = release.run(&fx, phase);
            assert!(
                outcome.code != Some(0)
                    && outcome.stderr.contains("rust_toolchain_bytes_mismatch")
                    && !outcome.stdout.contains("verified file by file"),
                "{file} {phase:?}: {outcome:?}"
            );
        }
    }
}

#[test]
fn b7_a_lone_toolchain_of_another_host_triple_is_verified_file_by_file_not_hard_failed_by_the_pin()
{
    for triple in [
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-musl",
    ] {
        for phase in PHASES {
            let (fx, release) = fixture_with(&Release::installed);
            fx.rename_root(&format!("{VERSION}-{triple}"));
            let outcome = release.run_pinned(&fx, phase, &[fixture_pin()]);
            assert!(
                outcome.code == Some(0)
                    && outcome.stdout.contains("verified file by file")
                    && !outcome.combined().contains(MISMATCH)
                    && !outcome.combined().contains("toolchains_differ")
                    && !outcome.combined().contains("tree_unreadable"),
                "{triple} {phase:?}: {outcome:?}"
            );
            let (tampered, release) = fixture_with(&Release::installed);
            tampered.put("lib/x.so", b"sO\n", 0o644);
            tampered.rename_root(&format!("{VERSION}-{triple}"));
            let flipped = release.run_pinned(&tampered, phase, &[fixture_pin()]);
            assert!(
                flipped.code != Some(0)
                    && flipped.stderr.contains("rust_toolchain_bytes_mismatch")
                    && !flipped.stdout.contains("verified file by file"),
                "{triple} {phase:?}: {flipped:?}"
            );
        }
    }
}

const EVIL_RUSTC: &[u8] = b"evil rustc\n";

fn forge_sums_for_the_tampered_rustc(fx: &Fixture, release: &Release, through_link: bool) {
    let sums = fx
        .verified_dir()
        .join(format!("{COMPONENT}.{}", release.tarball_sha256));
    let honest = fs::read_to_string(&sums).expect("sums stored by the honest run");
    let forged = honest
        .lines()
        .map(|line| match line.strip_suffix("  ./bin/rustc") {
            Some(_) => format!("{}  ./bin/rustc", sha256_hex(EVIL_RUSTC)),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(forged.trim_end(), honest.trim_end());
    fs::write(&sums, format!("{forged}\n")).expect("plant forged sums");
    if through_link {
        let outside = fx.runner_temp().join("outside");
        fs::rename(fx.verified_dir(), &outside).expect("move verified dir");
        symlink(&outside, fx.verified_dir()).expect("link verified dir");
    }
    fx.put("bin/rustc", EVIL_RUSTC, 0o755);
}

#[test]
fn c1_the_restored_phase_never_trusts_component_sums_it_did_not_compute() {
    for through_link in [false, true] {
        let (fx, release) = fixture_with(&Release::installed);
        release
            .run(&fx, ToolchainPhase::Restored)
            .assert_fallback_passed();
        forge_sums_for_the_tampered_rustc(&fx, &release, through_link);
        let outcome = release.run(&fx, ToolchainPhase::Restored);
        assert!(
            outcome.code == Some(1)
                && outcome.stderr.contains("rust_toolchain_bytes_mismatch")
                && !outcome.stdout.contains("verified file by file"),
            "through_link={through_link}: {outcome:?}"
        );
    }

    let (pristine, release) = fixture_with(&Release::installed);
    let sums = pristine
        .verified_dir()
        .join(format!("{COMPONENT}.{}", release.tarball_sha256));
    fs::create_dir_all(pristine.verified_dir()).expect("plant dir");
    fs::write(&sums, format!("{}  ./bin/rustc\n", "0".repeat(64))).expect("plant junk sums");
    release
        .run(&pristine, ToolchainPhase::Restored)
        .assert_fallback_passed();
}

#[test]
fn c2_the_installed_phase_reuses_what_the_restored_phase_verified_without_the_network() {
    let (fx, release) = fixture_with(&Release::installed);
    release
        .run(&fx, ToolchainPhase::Restored)
        .assert_fallback_passed();
    let outcome = release.run(&fx, ToolchainPhase::Installed);
    assert!(
        outcome.code == Some(0)
            && outcome.stdout.contains("verified file by file")
            && outcome.requests == [CHANNEL_URL, TARBALL_URL],
        "{outcome:?}"
    );
}

fn fail_closed(outcome: &Outcome, code: &str) -> bool {
    outcome.code == Some(1)
        && outcome.stderr.contains(code)
        && !outcome.stdout.contains("verified file by file")
}

#[test]
fn r1_the_fallback_refuses_a_release_whose_sources_are_not_the_catalogued_ones() {
    for phase in PHASES {
        let (substituted, release) = fixture_with(&Release::installed);
        substituted.put("bin/rustc", EVIL_RUSTC, 0o755);
        Release::of(&substituted);
        substituted.serve_channel(release.channel());
        let outcome = release.run(&substituted, phase);
        assert!(
            fail_closed(&outcome, "rust_component_tarball_digest_mismatch"),
            "substituted tarball {phase:?}: {outcome:?}"
        );
        let stored = substituted
            .verified_dir()
            .join(format!("{COMPONENT}.{}", release.tarball_sha256));
        assert!(!stored.exists(), "substituted tarball {phase:?}");

        let offsite = FIXTURE_CHANNEL.replace(
            "https://static.rust-lang.org/dist/2026-10-15/cargo-1.99.1-x86_64-unknown-linux-gnu.tar.xz",
            "https://attacker.example/cargo.tar.xz",
        );
        let fx = Fixture::with_components(&format!("{COMPONENT}\n"));
        fx.serve_channel(offsite.as_bytes());
        let outcome = fx.run(&script_with(offsite.as_bytes(), phase, &[]));
        assert!(
            fail_closed(&outcome, "rust_component_url_untrusted")
                && outcome.requests == [CHANNEL_URL],
            "off-site component url {phase:?}: {outcome:?}"
        );

        let fx = Fixture::standard();
        let uncatalogued = toolchain_script(
            VERSION,
            phase,
            &[("1.98.0", &"0".repeat(64), "2026-01-01")],
            TreeCheck::FileByFile,
        );
        let outcome = fx.run(&uncatalogued);
        assert!(
            fail_closed(&outcome, "rust_channel_manifest_not_catalogued")
                && outcome.requests.is_empty(),
            "uncatalogued version {phase:?}: {outcome:?}"
        );

        let fx = Fixture::with_components(&format!("{FIXTURE_COMPONENTS}rust-docs-x\n"));
        let outcome = fx.run(&script(phase, &[]));
        assert!(
            fail_closed(&outcome, "rust_component_not_in_manifest"),
            "component the channel does not ship {phase:?}: {outcome:?}"
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn g2_the_fallback_ignores_tar_and_xz_environment_options() {
    let poison = [
        ("XZ_DEFAULTS", "--memlimit=1B"),
        ("TAR_OPTIONS", "--exclude=*"),
    ];
    for phase in PHASES {
        let (fx, release) = fixture_with(&Release::installed);
        let outcome = release.run_file_by_file_with_env(&fx, phase, &poison);
        assert!(
            outcome.code == Some(0) && outcome.stdout.contains("verified file by file"),
            "{phase:?}: {outcome:?}"
        );
    }
}
