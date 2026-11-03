// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::process::CommandExt;
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

const HOSTILE_ENV: [(&str, &str); 3] = [
    ("QUOTING_STYLE", "c"),
    ("LC_ALL", "tr_TR.UTF-8"),
    ("XZ_DEFAULTS", "--memlimit=1B"),
];

#[test]
fn h2_a_preset_marker_does_not_let_the_callers_environment_reach_the_tools() {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let ran = scratch.path().join("ran");
    let hook = scratch.path().join("hook");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\nprintf hook >> {}\n", ran.display()),
    )
    .expect("write hook");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let tar_options = format!("--checkpoint=1 --checkpoint-action=exec={}", hook.display());

    let probe = scratch.path().join("probe.tar");
    let packed = Command::new("tar")
        .args([
            "-cf".as_ref(),
            probe.as_os_str(),
            "-C".as_ref(),
            scratch.path().as_os_str(),
            "hook".as_ref(),
        ])
        .status()
        .expect("pack probe");
    assert!(packed.success());
    let listed = Command::new("tar")
        .args(["-tf".as_ref(), probe.as_os_str()])
        .env("TAR_OPTIONS", &tar_options)
        .output()
        .expect("list probe");
    assert!(listed.status.success());
    assert_eq!(std::fs::read_to_string(&ran).unwrap_or_default(), "hook");
    std::fs::remove_file(&ran).expect("reset probe");

    let bundle = Bundle::new(&|_| {});
    let clean = bundle.derive_with_env(&[]);
    assert_eq!(clean.code, Some(0), "{clean:?}");
    let hostile: Vec<(&str, &str)> = HOSTILE_ENV
        .into_iter()
        .chain([("TAR_OPTIONS", tar_options.as_str())])
        .collect();
    for preset_marker in [false, true] {
        let mut env = hostile.clone();
        if preset_marker {
            env.push(("VELNOR_DERIVE_CLEAN", "1"));
        }
        let derived = bundle.derive_with_env(&env);
        assert_eq!(
            std::fs::read_to_string(&ran).unwrap_or_default(),
            "",
            "marker preset {preset_marker}: {derived:?}"
        );
        assert_eq!(
            derived.code,
            Some(0),
            "marker preset {preset_marker}: {derived:?}"
        );
        assert_eq!(
            derived.stdout, clean.stdout,
            "marker preset {preset_marker}"
        );
    }

    let fx = Fixture::standard();
    let mut command = Command::new(crate::pinned::derive_tool());
    command
        .args(["--tree".as_ref(), fx.root().as_os_str()])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("VELNOR_DERIVE_CLEAN", "1")
        .envs(hostile.iter().copied());
    let tree = outcome(&command.output().expect("run derive tool"));
    assert_eq!(tree.code, Some(0), "{tree:?}");
    assert!(
        tree.stdout
            .contains(&format!("components_sha256={FIXTURE_COMPONENTS_SHA256}\n"))
            && tree
                .stdout
                .contains(&format!("tree_sha256={FIXTURE_TREE_SHA256}\n")),
        "{tree:?}"
    );
}

const EMPTY_INPUT_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const NOBODY: u32 = 65534;

fn derived_fixture(outcome: &Outcome) -> bool {
    outcome.code == Some(0)
        && outcome
            .stdout
            .contains(&format!("components_sha256={FIXTURE_COMPONENTS_SHA256}\n"))
        && outcome
            .stdout
            .contains(&format!("tree_sha256={FIXTURE_TREE_SHA256}\n"))
}

fn faithful(outcome: &Outcome) -> bool {
    !outcome.stdout.contains(EMPTY_INPUT_SHA256)
        && (derived_fixture(outcome) || failed_closed(outcome))
}

fn relative_tree(fx: &Fixture, name: &str, home: &Fixture) -> Outcome {
    outcome(
        &Command::new(crate::pinned::derive_tool())
            .current_dir(fx.toolchains())
            .env("HOME", home.root())
            .args(["--tree", name])
            .output()
            .expect("run derive tool"),
    )
}

#[test]
fn r9a_a_dash_named_relative_tree_is_the_tree_named_or_refused() {
    let decoy = Fixture::standard();
    decoy.put("lib/decoy", b"decoy\n", 0o644);
    let mut unfaithful = Vec::new();
    for name in ["plain", "-P", "--x"] {
        let fx = Fixture::standard();
        fx.rename_root(name);
        let control = derive(&["--tree".as_ref(), fx.toolchains().join(name).as_os_str()]);
        assert!(derived_fixture(&control), "{name}: {control:?}");
        let relative = relative_tree(&fx, name, &decoy);
        if !faithful(&relative) {
            unfaithful.push(format!("{name}: {relative:?}"));
        }
    }
    let absent = Fixture::standard();
    let missing = relative_tree(&absent, "-P", &decoy);
    if !failed_closed(&missing) || missing.stdout.contains(EMPTY_INPUT_SHA256) {
        unfaithful.push(format!("absent -P: {missing:?}"));
    }
    assert!(unfaithful.is_empty(), "{unfaithful:#?}");
}

fn running_privileged() -> bool {
    let probe = tempfile::tempdir().expect("probe dir");
    let file = probe.path().join("sealed");
    std::fs::write(&file, b"sealed").expect("write probe");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).expect("seal probe");
    std::fs::File::open(&file).is_ok()
}

fn derive_tree_as_unprivileged_reader(fx: &Fixture) -> Outcome {
    let mut command = Command::new(crate::pinned::derive_tool());
    command.args(["--tree".as_ref(), fx.root().as_os_str()]);
    if running_privileged() {
        let toolchains = fx.toolchains();
        let rustup = fx.rustup_home();
        let home = rustup.parent().expect("fixture home");
        for dir in [home, rustup.as_path(), toolchains.as_path()] {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))
                .expect("open ancestor");
        }
        command.uid(NOBODY).gid(NOBODY);
    }
    outcome(&command.output().expect("run derive tool"))
}

#[test]
fn r9b_an_unreadable_input_fails_closed_never_the_empty_input_digest() {
    let control = derive_tree_as_unprivileged_reader(&Fixture::standard());
    assert!(derived_fixture(&control), "{control:?}");
    let unreadable = ["lib/rustlib/components", "lib/x.so"]
        .into_iter()
        .filter_map(|path| {
            let fx = Fixture::standard();
            fx.chmod(path, 0o000);
            let outcome = derive_tree_as_unprivileged_reader(&fx);
            let closed = failed_closed(&outcome) && !outcome.stdout.contains(EMPTY_INPUT_SHA256);
            (!closed).then(|| format!("{path}: {outcome:?}"))
        })
        .collect::<Vec<_>>();
    assert!(unreadable.is_empty(), "{unreadable:#?}");
}
