// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::verifier_builders::{ToolchainPhase, TreeDigestPin};

use std::path::Path;

use crate::fixture::{
    CAP, CHANNEL_MANIFEST, FIXTURE_BOOKKEEPING, FIXTURE_COMPONENTS, FIXTURE_COMPONENTS_SHA256,
    Fixture, INSTALLER_VERSION, MALFORMED_HEADERS, MANIFEST_CARGO, MANIFEST_RUSTC, MISMATCH,
    MULTIRUST_CONFIG, Outcome, PHASES, Row, TARGET, VERSION, fixture_pin, row, run_rows, script,
};

const COMPONENTS: &str = "lib/rustlib/components";

const LISTING_GUARD: &str = "listing guard";
const UNSAFE_NAME_CHECK: &str = "unsafe-name check";

const UNSAFE_NAMES: [(&str, &str, &str); 4] = [
    (
        "a b",
        LISTING_GUARD,
        "e103c304a93bba11dbfe7cb9608f3b6039ce28ff807aa4ed29759435ad07c3ef",
    ),
    (
        "a$b",
        UNSAFE_NAME_CHECK,
        "8d9288872dbb19602babe083849419080568ca0c7516a7cea114e997240084d0",
    ),
    (
        "a*b",
        UNSAFE_NAME_CHECK,
        "7d1b42cab88c8754aa08a8ccc1d6c7459fb2ac3de95933667cbd211942fbd6e0",
    ),
    (
        "a\nb",
        LISTING_GUARD,
        "0515b57e2fa3843d16787185cfbc0149d5f2c00872c93ef3d64c40b3835ef3a0",
    ),
];

fn hard_failed(outcome: &Outcome) -> bool {
    outcome.hard_failed(MISMATCH)
}

#[test]
fn n1a_any_entry_mutation_under_the_matching_pin_fails_the_job() {
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
        row("bin/rustc not executable", &|fx| {
            fx.chmod("bin/rustc", 0o644);
        }),
    ];
    run_rows(&rows, &hard_failed);
}

#[test]
fn n1b_any_bookkeeping_mutation_under_the_matching_pin_fails_the_job() {
    let rows = [
        row("poisoned bookkeeping manifest", &|fx| {
            let evil = FIXTURE_BOOKKEEPING.replace(
                "cargo-1.99.1-x86_64-unknown-linux-gnu.tar.xz",
                "evil.tar.xz",
            );
            fx.put(CHANNEL_MANIFEST, evil.as_bytes(), 0o644);
        }),
        row("unspaced bookkeeping assignment", &|fx| {
            let evil = format!("{FIXTURE_BOOKKEEPING}xz_url=\"https://attacker.example/x\"\n");
            fx.put(CHANNEL_MANIFEST, evil.as_bytes(), 0o644);
        }),
        row("missing bookkeeping manifest", &|fx| {
            fx.remove(CHANNEL_MANIFEST);
        }),
        row("cargo manifest bytes changed", &|fx| {
            fx.put(MANIFEST_CARGO, b"file:lib/x.sO\nfile:share/.dot\n", 0o644);
        }),
        row("rustc manifest bytes changed", &|fx| {
            fx.put(MANIFEST_RUSTC, b"file:bin/rusTc\n", 0o644);
        }),
        row("installer version 3 to 4", &|fx| {
            fx.put(INSTALLER_VERSION, b"4", 0o644);
        }),
        row("extra component manifest", &|fx| {
            fx.put("lib/rustlib/manifest-evil", b"", 0o644);
        }),
        row("cargo manifest removed", &|fx| fx.remove(MANIFEST_CARGO)),
        row("multirust config removed", &|fx| {
            fx.remove(MULTIRUST_CONFIG);
        }),
        row("multirust config over the cap", &|fx| {
            fx.put(MULTIRUST_CONFIG, &vec![b'x'; CAP + 1], 0o644);
        }),
        row("rename appended to the channel manifest", &|fx| {
            let evil =
                format!("{FIXTURE_BOOKKEEPING}\n[renames.rustc]\nto = \"rustfmt-preview\"\n");
            fx.put(CHANNEL_MANIFEST, evil.as_bytes(), 0o644);
        }),
        row("rename target changed in the channel manifest", &|fx| {
            let evil =
                FIXTURE_BOOKKEEPING.replace("to = \"clippy-preview\"", "to = \"rustfmt-preview\"");
            fx.put(CHANNEL_MANIFEST, evil.as_bytes(), 0o644);
        }),
        row("unknown key in a package target table", &|fx| {
            let evil = FIXTURE_BOOKKEEPING.replacen(
                "available = true\n",
                "available = true\nsource = \"https://attacker.example/\"\n",
                1,
            );
            fx.put(CHANNEL_MANIFEST, evil.as_bytes(), 0o644);
        }),
        row("unknown table appended to the channel manifest", &|fx| {
            let evil = format!("{FIXTURE_BOOKKEEPING}\n[mirrors]\n");
            fx.put(CHANNEL_MANIFEST, evil.as_bytes(), 0o644);
        }),
    ];
    run_rows(&rows, &hard_failed);
}

#[test]
fn n1c_a_changed_selection_input_under_the_matching_pin_fails_the_job() {
    let rows = [
        row("extra empty toolchains entry", &|fx| fx.sibling("zz")),
        row("second same-version toolchain", &|fx| {
            fx.sibling("1.99.1-aarch64-unknown-linux-gnu");
        }),
        row("dot toolchains entry", &|fx| fx.sibling(".hidden")),
        row("toolchain dir renamed", &|fx| fx.rename_root("1.99.1-evil")),
        row("components cut to an unpinned set", &|fx| {
            fx.put(COMPONENTS, b"cargo-x86_64-unknown-linux-gnu\n", 0o644);
        }),
        row("components line duplicated", &|fx| {
            let doubled = format!("{FIXTURE_COMPONENTS}rustc-x86_64-unknown-linux-gnu\n");
            fx.put(COMPONENTS, doubled.as_bytes(), 0o644);
        }),
        row("components file empty", &|fx| {
            fx.put(COMPONENTS, b"", 0o644);
        }),
    ];
    run_rows(&rows, &hard_failed);
}

#[test]
fn n2_unsafe_path_characters_fail_the_job_under_a_pinned_components_set() {
    let mutations = UNSAFE_NAMES.map(|(name, _, _)| {
        move |fx: &Fixture| {
            fx.put(&format!("lib/{name}"), b"x", 0o644);
        }
    });
    let rows = UNSAFE_NAMES
        .iter()
        .zip(&mutations)
        .map(|((name, guard, literal), mutate)| Row {
            name: format!("{name:?} ({guard})"),
            mutate,
            pin: TreeDigestPin::new(VERSION, TARGET, FIXTURE_COMPONENTS_SHA256, literal),
        })
        .collect::<Vec<_>>();
    run_rows(&rows, &hard_failed);
}

const ARTIFACTS_BODY: &str = "url = \"https://attacker.example/x\"\n";

#[test]
fn n1d_a_malformed_section_header_is_unreadable_while_a_well_formed_artifacts_header_is_skipped() {
    let well_formed = format!("{FIXTURE_BOOKKEEPING}\n[[artifacts.foo]]\n{ARTIFACTS_BODY}");
    let malformed = format!("{FIXTURE_BOOKKEEPING}\n[[artifacts.foo]\n{ARTIFACTS_BODY}");
    for phase in PHASES {
        let accepted = Fixture::standard();
        accepted.put(CHANNEL_MANIFEST, well_formed.as_bytes(), 0o644);
        let outcome = accepted.run(&script(phase, &[fixture_pin()]));
        assert!(outcome.is_pinned(), "well-formed {phase:?}: {outcome:?}");
        let rejected = Fixture::standard();
        rejected.put(CHANNEL_MANIFEST, malformed.as_bytes(), 0o644);
        let outcome = rejected.run(&script(phase, &[fixture_pin()]));
        assert!(
            hard_failed(&outcome) && outcome.stdout.contains("bookkeeping_manifest_unreadable"),
            "malformed {phase:?}: {outcome:?}"
        );
    }
}

#[test]
fn n1e_a_section_header_outside_the_dotted_key_grammar_is_unreadable_in_both_phases() {
    let mut failures = Vec::new();
    for header in MALFORMED_HEADERS {
        let manifest = format!("{FIXTURE_BOOKKEEPING}\n{header}\n{ARTIFACTS_BODY}");
        for phase in PHASES {
            let fx = Fixture::standard();
            fx.put(CHANNEL_MANIFEST, manifest.as_bytes(), 0o644);
            let outcome = fx.run(&script(phase, &[fixture_pin()]));
            if !(hard_failed(&outcome)
                && outcome.stdout.contains("bookkeeping_manifest_unreadable"))
            {
                failures.push(format!("{header} {phase:?}: {outcome:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn config_with(bytes: usize) -> Fixture {
    let fx = Fixture::standard();
    fx.put(MULTIRUST_CONFIG, &vec![b'x'; bytes], 0o644);
    fx
}

fn run_env(fx: &Fixture, phase: ToolchainPhase, env: (&str, &str)) -> Outcome {
    fx.run_with_env(
        &script(phase, &[fixture_pin()]),
        Path::new("/usr/bin"),
        &[env],
    )
}

#[test]
fn n10_the_size_cap_holds_under_gnu_block_size_variables() {
    for env in [("BLOCK_SIZE", "1M"), ("LS_BLOCK_SIZE", "1M")] {
        for phase in PHASES {
            let over = run_env(&config_with(CAP + 1), phase, env);
            assert!(
                hard_failed(&over) && over.stdout.contains("listing_failed"),
                "{env:?} {phase:?} over the cap: {over:?}"
            );
            let at_cap = run_env(&config_with(CAP), phase, env);
            assert!(
                at_cap.is_pinned(),
                "{env:?} {phase:?} at the cap: {at_cap:?}"
            );
        }
    }
}
