// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::os::unix::fs::symlink;
use std::process::Command;

use crate::derive::outcome;
use crate::fixture::{
    CAP, CHANNEL_MANIFEST, FIXTURE_CHANNEL, Fixture, MISMATCH, MULTIRUST_CONFIG, Outcome, TARGET,
    VERSION, fixture_pin, script,
};
use velnor_actions_workflow_renderer::verifier_builders::ToolchainPhase;

fn derive(args: &[&std::ffi::OsStr]) -> Outcome {
    outcome(
        &Command::new(crate::pinned::derive_tool())
            .args(args)
            .output()
            .expect("run derive tool"),
    )
}

fn derive_tree(fx: &Fixture) -> Outcome {
    derive(&["--tree".as_ref(), fx.root().as_os_str()])
}

fn failed_closed(outcome: &Outcome) -> bool {
    outcome.code.is_some_and(|code| code != 0)
        && !outcome.stdout.contains("tree_sha256=")
        && outcome.stderr.starts_with("derive-rust-tree-digest:")
}

fn tree_digest(outcome: &Outcome) -> &str {
    outcome
        .stdout
        .lines()
        .find(|line| line.starts_with("tree_sha256="))
        .unwrap_or_else(|| panic!("no tree_sha256 in {outcome:?}"))
}

fn with_config(bytes: usize) -> Fixture {
    let fx = Fixture::standard();
    fx.put(MULTIRUST_CONFIG, &vec![b'x'; bytes], 0o644);
    fx
}

#[test]
fn n9_the_config_cap_fails_closed_and_the_manifest_has_no_cap() {
    let control = derive_tree(&with_config(CAP));
    assert_eq!(control.code, Some(0), "{control:?}");
    let over = derive_tree(&with_config(CAP + 1));
    assert!(failed_closed(&over), "{over:?}");

    let mut padded = FIXTURE_CHANNEL.to_owned();
    padded.push_str(&"\n".repeat(CAP + 1 - padded.len()));
    let big = Fixture::standard();
    big.put(CHANNEL_MANIFEST, padded.as_bytes(), 0o644);
    let derived = derive_tree(&big);
    assert_eq!(derived.code, Some(0), "{derived:?}");
    let plain = derive_tree(&Fixture::standard());
    assert_ne!(tree_digest(&derived), tree_digest(&plain));
    let restored = big.run(&script(ToolchainPhase::Restored, &[fixture_pin()]));
    restored.assert_hard_failed(MISMATCH);
    assert!(restored.stdout.contains("tree_differs"), "{restored:?}");
}

#[test]
fn d1_derive_has_no_listing_option_and_never_writes_into_a_tree() {
    let fx = Fixture::standard();
    fx.put("lib/prior.listing", b"earlier run\n", 0o644);
    let outside = tempfile::tempdir().expect("outside dir");
    let alias = outside.path().join("alias");
    std::fs::hard_link(fx.root().join("lib/prior.listing"), &alias).expect("hard link");
    let tree_link = outside.path().join("tree");
    symlink(fx.root(), &tree_link).expect("tree link");
    let targets = [
        fx.root().join("out.listing"),
        fx.root().join("lib/prior.listing"),
        alias.clone(),
        tree_link,
        outside.path().join("fresh.listing"),
    ];
    let before = fx.snapshot();
    let plain = derive_tree(&fx);
    assert_eq!(plain.code, Some(0), "{plain:?}");
    for target in &targets {
        for args in [
            vec![
                "--listing".as_ref(),
                target.as_os_str(),
                "--tree".as_ref(),
                fx.root().as_os_str(),
            ],
            vec![
                "--from-manifest".as_ref(),
                VERSION.as_ref(),
                TARGET.as_ref(),
                "cargo-x86_64-unknown-linux-gnu".as_ref(),
                "--listing".as_ref(),
                target.as_os_str(),
            ],
        ] {
            let outcome = derive(&args);
            assert!(
                outcome.code == Some(2)
                    && !outcome.stdout.contains("sha256=")
                    && outcome.stderr.contains("usage:"),
                "{target:?}: {outcome:?}"
            );
            assert_eq!(fx.snapshot(), before, "{target:?}");
        }
    }
    assert!(!outside.path().join("fresh.listing").exists());
    assert_eq!(std::fs::read(&alias).expect("read alias"), b"earlier run\n");
    let again = derive_tree(&fx);
    assert_eq!(again.stdout, plain.stdout);
    assert_eq!(fx.snapshot(), before);
}
