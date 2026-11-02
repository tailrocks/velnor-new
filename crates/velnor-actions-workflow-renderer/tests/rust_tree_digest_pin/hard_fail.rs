// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::path::Path;

use velnor_actions_workflow_renderer::verifier_builders::ToolchainPhase;

use crate::fixture::{
    CAP, FIXTURE_COMPONENTS, Fixture, INSTALLER_VERSION, MANIFEST_CARGO, MANIFEST_RUSTC, MISMATCH,
    MULTIRUST_CONFIG, Outcome, PHASES, Row, fixture_pin, row, run_rows, script,
};

const COMPONENTS: &str = "lib/rustlib/components";

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
    let names = ["a b", "a$b", "a*b", "a\nb"];
    let mutations = names.map(|name| {
        move |fx: &Fixture| {
            fx.put(&format!("lib/{name}"), b"x", 0o644);
        }
    });
    let rows = names
        .iter()
        .zip(&mutations)
        .map(|(name, mutate)| Row {
            name: format!("{name:?}"),
            mutate,
            pin: fixture_pin(),
        })
        .collect::<Vec<_>>();
    let accepts =
        |outcome: &Outcome| hard_failed(outcome) && outcome.stdout.contains("unsafe_name");
    run_rows(&rows, &accepts);
}

const POISON: [(&str, &str); 7] = [
    ("QUOTING_STYLE", "c"),
    ("QUOTING_STYLE", "locale"),
    ("BLOCK_SIZE", "1M"),
    ("LS_BLOCK_SIZE", "1M"),
    ("TIME_STYLE", "long-iso"),
    ("POSIXLY_CORRECT", "1"),
    ("LC_ALL", "en_US.UTF-8"),
];

fn run_env(fx: &Fixture, phase: ToolchainPhase, env: &[(&str, &str)]) -> Outcome {
    fx.run_with_env(&script(phase, &[fixture_pin()]), Path::new("/usr/bin"), env)
}

fn config_with(bytes: usize) -> Fixture {
    let fx = Fixture::standard();
    fx.put(MULTIRUST_CONFIG, &vec![b'x'; bytes], 0o644);
    fx
}

#[test]
fn g1_the_verdict_does_not_depend_on_the_caller_environment() {
    let baseline = PHASES.map(|phase| {
        let outcome = run_env(&Fixture::standard(), phase, &[]);
        outcome.assert_pinned();
        outcome.stdout
    });
    for env in POISON {
        let fx = Fixture::standard();
        for (phase, expected) in PHASES.into_iter().zip(&baseline) {
            let outcome = run_env(&fx, phase, &[env]);
            assert!(
                outcome.is_pinned() && &outcome.stdout == expected,
                "{env:?} {phase:?}: {outcome:?}"
            );
        }
    }
    for phase in PHASES {
        let at_cap = run_env(&config_with(CAP), phase, &[("BLOCK_SIZE", "1M")]);
        assert!(at_cap.is_pinned(), "{phase:?} at the cap: {at_cap:?}");
        let over = run_env(&config_with(CAP + 1), phase, &[("BLOCK_SIZE", "1M")]);
        assert!(
            hard_failed(&over) && over.stdout.contains("listing_failed"),
            "{phase:?} over the cap: {over:?}"
        );
    }
}
