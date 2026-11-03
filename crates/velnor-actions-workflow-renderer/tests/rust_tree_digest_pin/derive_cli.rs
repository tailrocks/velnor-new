// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::os::unix::fs::{PermissionsExt, symlink};
use std::process::Command;

use crate::derive::{Bundle, outcome};
use crate::fixture::{
    CAP, CHANNEL_MANIFEST, CHANNEL_URL, FIXTURE_CHANNEL, FIXTURE_COMPONENTS_SHA256,
    FIXTURE_TREE_SHA256, Fixture, MISMATCH, MULTIRUST_CONFIG, Outcome, TARBALL_URL, TARGET,
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

const TREE_HELPERS: [&str; 10] = [
    "sha256sum",
    "shasum",
    "dirname",
    "mktemp",
    "rm",
    "find",
    "sort",
    "awk",
    "tr",
    "sed",
];

#[test]
fn h1_derive_runs_no_helper_from_the_callers_path() {
    let fx = Fixture::standard();
    let hostile = tempfile::tempdir().expect("hostile dir");
    let marker = hostile.path().join("ran");
    for helper in TREE_HELPERS {
        let lies = matches!(helper, "sha256sum" | "shasum");
        let body = format!(
            "#!/bin/sh\nprintf '%s\\n' {helper} >> {marker}\n{}",
            if lies {
                format!("printf '%s  -\\n' {}\n", "a".repeat(64))
            } else {
                format!("PATH=/usr/bin:/bin\nexport PATH\nexec {helper} \"$@\"\n")
            },
            marker = marker.display(),
        );
        let path = hostile.path().join(helper);
        std::fs::write(&path, body).expect("write fake");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    for preset_marker in [false, true] {
        let mut command = Command::new(crate::pinned::derive_tool());
        command
            .args(["--tree".as_ref(), fx.root().as_os_str()])
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", hostile.path().display()),
            );
        if preset_marker {
            command.env("VELNOR_DERIVE_CLEAN", "1");
        }
        let derived = outcome(&command.output().expect("run derive tool"));
        let ran = std::fs::read_to_string(&marker).unwrap_or_default();
        assert_eq!(ran, "", "marker preset {preset_marker}: {derived:?}");
        assert_eq!(derived.code, Some(0), "{derived:?}");
        assert!(
            derived
                .stdout
                .contains(&format!("components_sha256={FIXTURE_COMPONENTS_SHA256}\n"))
                && derived
                    .stdout
                    .contains(&format!("tree_sha256={FIXTURE_TREE_SHA256}\n")),
            "marker preset {preset_marker}: {derived:?}"
        );
    }
}

#[test]
fn v1_a_blank_version_or_target_is_refused_before_any_download() {
    let bundle = Bundle::new(&|_| {});
    let control = bundle.derive_with_args(VERSION, TARGET);
    assert_eq!(control.code, Some(0), "{control:?}");
    assert!(
        control
            .stdout
            .contains(&format!("TreeDigestPin::new(\"{VERSION}\", \"{TARGET}\", ")),
        "{control:?}"
    );
    assert_eq!(control.requests, [CHANNEL_URL, TARBALL_URL], "{control:?}");
    let refused = [("blank target", VERSION, ""), ("blank version", "", TARGET)]
        .into_iter()
        .filter_map(|(name, version, target)| {
            let outcome = Bundle::new(&|_| {}).derive_with_args(version, target);
            let closed = matches!(outcome.code, Some(1 | 2))
                && outcome.stdout.is_empty()
                && !outcome.stderr.is_empty()
                && outcome.requests.is_empty();
            (!closed).then(|| format!("{name}: {outcome:?}"))
        })
        .collect::<Vec<_>>();
    assert!(refused.is_empty(), "{refused:#?}");
}
