// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use velnor_actions_workflow_renderer::verifier_builders::ToolchainPhase;

use crate::fixture::{CHANNEL_MANIFEST, Fixture, PHASES, fixture_pin, script};
use crate::outcome::{Node, collect};
use crate::release::rustup_bookkeeping;

const CODE: &str = "rust_toolchain_path_unexpected";
const SCANNED: &str = "rust_toolchain_file_unexpected";

fn external(fx: &Fixture) -> PathBuf {
    fx.rustup_home().with_file_name("external")
}

fn relocate(fx: &Fixture, linked: &Path) {
    let outside = external(fx);
    fs::rename(linked, &outside).expect("move outside");
    symlink(&outside, linked).expect("link back");
}

fn stale(fx: &Fixture) {
    fx.put(
        CHANNEL_MANIFEST,
        rustup_bookkeeping(&"5".repeat(64)).as_bytes(),
        0o644,
    );
}

fn tree(path: &Path) -> Vec<Node> {
    let mut nodes = Vec::new();
    collect(path, path, &mut nodes);
    nodes.sort_by(|a, b| a.path().cmp(b.path()));
    nodes
}

type Row<'a> = (&'a str, &'a [&'a str], &'a (dyn Fn(&Fixture) + Sync));

#[test]
fn a1_a_linked_ancestor_of_the_toolchain_is_refused_and_the_target_is_untouched() {
    let rows: [Row<'_>; 6] = [
        ("toolchains", &[CODE], &|fx| relocate(fx, &fx.toolchains())),
        ("toolchains with a stale manifest", &[CODE], &|fx| {
            stale(fx);
            relocate(fx, &fx.toolchains());
        }),
        ("toolchains with a foreign entry", &[CODE], &|fx| {
            stale(fx);
            fx.sibling("zz");
            relocate(fx, &fx.toolchains());
        }),
        ("toolchains to an empty directory", &[CODE], &|fx| {
            fs::remove_dir_all(fx.toolchains()).expect("remove toolchains");
            fs::create_dir(external(fx)).expect("empty outside");
            symlink(external(fx), fx.toolchains()).expect("link");
        }),
        ("lib", &[CODE, SCANNED], &|fx| {
            stale(fx);
            relocate(fx, &fx.root().join("lib"));
        }),
        ("lib/rustlib", &[CODE, SCANNED], &|fx| {
            stale(fx);
            relocate(fx, &fx.root().join("lib/rustlib"));
        }),
    ];
    let pins = [fixture_pin()];
    let mut failures = Vec::new();
    for (name, codes, mutate) in rows {
        for phase in PHASES {
            for (flavour, pins) in [("pinned", &pins[..]), ("unpinned", &[][..])] {
                let fx = Fixture::standard();
                mutate(&fx);
                let before = tree(&external(&fx));
                let outcome = fx.run(&script(phase, pins));
                let changed = tree(&external(&fx)) != before;
                if !codes.iter().any(|code| outcome.hard_failed(code)) || changed {
                    failures.push(format!(
                        "{name} {phase:?} {flavour}: external changed {changed}, {outcome:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn a2_an_absent_toolchains_directory_is_still_a_fresh_restore() {
    let fx = Fixture::standard();
    fs::remove_dir_all(fx.toolchains()).expect("remove toolchains");
    let outcome = fx.run(&script(ToolchainPhase::Restored, &[fixture_pin()]));
    assert!(
        outcome.code == Some(0)
            && outcome.stdout.contains("no restored Rust")
            && outcome.requests.is_empty()
            && !fx.toolchains().exists(),
        "{outcome:?}"
    );
}
