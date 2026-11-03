// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::symlink;

use velnor_actions_workflow_renderer::verifier_builders::ToolchainPhase;

use crate::fixture::{
    CAP, CHANNEL_MANIFEST, CHANNEL_URL, FIXTURE_CHANNEL, Fixture, MISMATCH, PHASES, fixture_pin,
    script,
};
use crate::release::rustup_bookkeeping;

type Change = Option<(Vec<u8>, u32)>;

fn rewritten() -> String {
    rustup_bookkeeping(&"5".repeat(64))
}

fn pinned(phase: ToolchainPhase) -> String {
    script(phase, &[fixture_pin()])
}

fn assert_channel_bytes(fx: &Fixture) {
    assert_eq!(fx.read(CHANNEL_MANIFEST), FIXTURE_CHANNEL.as_bytes());
    assert_eq!(fx.mode(CHANNEL_MANIFEST), 0o644);
}

#[test]
fn px1_a_restored_match_leaves_a_verified_copy_so_the_installed_repair_stays_offline() {
    let fx = Fixture::standard();
    fx.run(&pinned(ToolchainPhase::Restored))
        .assert_offline_pinned();
    fx.put(CHANNEL_MANIFEST, rewritten().as_bytes(), 0o644);
    fx.run(&pinned(ToolchainPhase::Installed))
        .assert_offline_pinned();
    assert_channel_bytes(&fx);
}

#[test]
fn px2_the_installed_phase_alone_downloads_the_channel_once_and_binds_the_manifest() {
    let fx = Fixture::standard();
    fx.put(CHANNEL_MANIFEST, rewritten().as_bytes(), 0o644);
    let outcome = fx.run(&pinned(ToolchainPhase::Installed));
    assert!(outcome.is_pinned(), "{outcome:?}");
    assert_eq!(outcome.requests, [CHANNEL_URL], "{outcome:?}");
    assert_channel_bytes(&fx);
}

#[test]
fn px3_a_poisoned_job_local_copy_is_never_installed() {
    let fx = Fixture::standard();
    fx.run(&pinned(ToolchainPhase::Restored))
        .assert_offline_pinned();
    let stored = fs::read_dir(fx.verified_dir())
        .expect("verified dir")
        .map(|entry| entry.expect("entry").path())
        .collect::<Vec<_>>();
    assert_ne!(stored.len(), 0);
    for path in stored {
        let mut bytes = fs::read(&path).expect("read copy");
        bytes[0] ^= 1;
        fs::write(&path, bytes).expect("poison copy");
    }
    fx.put(CHANNEL_MANIFEST, rewritten().as_bytes(), 0o644);
    let outcome = fx.run(&pinned(ToolchainPhase::Installed));
    assert!(outcome.is_pinned(), "{outcome:?}");
    assert_eq!(outcome.requests, [CHANNEL_URL], "{outcome:?}");
    assert_channel_bytes(&fx);
}

#[test]
fn px4_the_overwrite_replaces_the_entry_and_leaves_nothing_behind() {
    let outside = tempfile::tempdir().expect("outside dir");
    let linked = Fixture::standard();
    linked.put(CHANNEL_MANIFEST, rewritten().as_bytes(), 0o644);
    let alias = outside.path().join("alias");
    fs::hard_link(linked.root().join(CHANNEL_MANIFEST), &alias).expect("hard link");
    linked
        .run(&pinned(ToolchainPhase::Installed))
        .assert_pinned();
    assert_eq!(
        fs::read(&alias).expect("read alias"),
        rewritten().as_bytes()
    );
    assert_channel_bytes(&linked);

    let masked = Fixture::standard();
    masked.put(CHANNEL_MANIFEST, rewritten().as_bytes(), 0o600);
    let under_umask = format!("umask 077; {}", pinned(ToolchainPhase::Installed));
    masked.run(&under_umask).assert_pinned();
    assert_channel_bytes(&masked);

    let clean = Fixture::standard();
    clean.put(CHANNEL_MANIFEST, rewritten().as_bytes(), 0o644);
    let paths = |fx: &Fixture| {
        fx.snapshot()
            .iter()
            .map(|node| node.path().to_owned())
            .collect::<Vec<_>>()
    };
    let before = paths(&clean);
    clean
        .run(&pinned(ToolchainPhase::Installed))
        .assert_pinned();
    assert_eq!(paths(&clean), before);
}

#[test]
fn b1_a_changed_manifest_fails_the_restored_phase_untouched_and_is_repaired_by_the_installed_phase()
{
    let mut padded = FIXTURE_CHANNEL.to_owned();
    padded.push_str(&"\n".repeat(CAP + 1 - padded.len()));
    let flipped = {
        let mut bytes = FIXTURE_CHANNEL.as_bytes().to_vec();
        bytes[0] ^= 1;
        bytes
    };
    let appended = format!("{FIXTURE_CHANNEL}\n[[artifacts.foo]]\nurl = \"x\n");
    let rows: [(&str, Change); 7] = [
        ("one-byte flip", Some((flipped, 0o644))),
        ("rustup rewrite", Some((rewritten().into_bytes(), 0o644))),
        (
            "invalid appended table",
            Some((appended.into_bytes(), 0o644)),
        ),
        ("padded past the cap", Some((padded.into_bytes(), 0o644))),
        ("truncated to empty", Some((Vec::new(), 0o644))),
        (
            "mode 0600",
            Some((FIXTURE_CHANNEL.as_bytes().to_vec(), 0o600)),
        ),
        ("removed", None),
    ];
    for (name, change) in rows {
        let apply = |fx: &Fixture| match &change {
            Some((bytes, mode)) => fx.put(CHANNEL_MANIFEST, bytes, *mode),
            None => fx.remove(CHANNEL_MANIFEST),
        };
        let restored = Fixture::standard();
        apply(&restored);
        let before = restored.snapshot();
        let outcome = restored.run(&pinned(ToolchainPhase::Restored));
        assert!(
            outcome.hard_failed(MISMATCH)
                && outcome.stdout.contains("tree_differs")
                && outcome.requests.is_empty(),
            "{name}: {outcome:?}"
        );
        assert_eq!(restored.snapshot(), before, "{name}");
        let installed = Fixture::standard();
        apply(&installed);
        let outcome = installed.run(&pinned(ToolchainPhase::Installed));
        assert!(outcome.is_pinned(), "{name}: {outcome:?}");
        assert_channel_bytes(&installed);
    }
}

#[test]
fn w1_a_download_that_is_not_the_catalogued_channel_changes_nothing_and_is_not_kept() {
    let pins = [fixture_pin()];
    for (name, pins) in [("pinned", &pins[..]), ("unpinned", &[][..])] {
        let fx = Fixture::standard();
        fx.put(CHANNEL_MANIFEST, rewritten().as_bytes(), 0o644);
        fx.serve_channel(b"not the catalogued channel\n");
        let before = fx.snapshot();
        let outcome = fx.run(&script(ToolchainPhase::Installed, pins));
        assert!(
            outcome.code == Some(1)
                && outcome
                    .stderr
                    .contains("rust_channel_manifest_digest_mismatch")
                && !outcome.combined().contains(MISMATCH),
            "{name}: {outcome:?}"
        );
        assert_eq!(fx.snapshot(), before, "{name}");
        fx.serve_channel(FIXTURE_CHANNEL.as_bytes());
        let retry = fx.run(&script(ToolchainPhase::Installed, pins));
        assert_eq!(retry.requests.len(), 2, "{name}: {retry:?}");
    }
}

#[test]
fn s1_the_overwrite_never_writes_through_a_link_or_into_a_directory() {
    let pins = [fixture_pin()];
    for phase in PHASES {
        for (name, pins) in [("pinned", &pins[..]), ("unpinned", &[][..])] {
            let outside = tempfile::tempdir().expect("outside dir");
            let target = outside.path().join("victim");
            fs::write(&target, b"victim\n").expect("victim");
            let fx = Fixture::standard();
            fx.remove(CHANNEL_MANIFEST);
            symlink(&target, fx.root().join(CHANNEL_MANIFEST)).expect("manifest link");
            let outcome = fx.run(&script(phase, pins));
            assert!(
                outcome.is_not_pinned()
                    && outcome.stderr.contains("rust_toolchain_file_unexpected"),
                "{name} {phase:?}: {outcome:?}"
            );
            assert_eq!(fs::read(&target).expect("read victim"), b"victim\n");

            let dir = Fixture::standard();
            dir.remove(CHANNEL_MANIFEST);
            dir.dir(CHANNEL_MANIFEST, 0o755);
            let outcome = dir.run(&script(phase, pins));
            assert!(
                outcome.is_not_pinned() && !outcome.combined().contains("verified file by file"),
                "{name} {phase:?} directory: {outcome:?}"
            );
            assert!(dir.root().join(CHANNEL_MANIFEST).is_dir());
            assert!(
                fs::read_dir(dir.root().join(CHANNEL_MANIFEST))
                    .expect("read manifest dir")
                    .next()
                    .is_none()
            );
        }
    }
}
