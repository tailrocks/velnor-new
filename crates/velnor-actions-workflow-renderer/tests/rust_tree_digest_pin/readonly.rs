// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;

use velnor_actions_workflow_renderer::verifier_builders::ToolchainPhase;

use crate::fixture::CHANNEL_MANIFEST;
use crate::release::{Release, fixture_with};

#[test]
fn readonly_file_by_file_verification_never_repairs_the_shared_manifest() {
    let (fx, release) = fixture_with(&Release::installed);
    fx.put(CHANNEL_MANIFEST, release.channel(), 0o644);
    let before = fx.snapshot();
    let manifest = fx.root().join(CHANNEL_MANIFEST);
    let inode = fs::metadata(&manifest).expect("manifest").ino();
    let outcome = release.run_file_by_file(&fx, ToolchainPhase::ReadOnly);
    assert!(
        outcome.code == Some(0) && outcome.stdout.contains("verified file by file"),
        "{outcome:?}"
    );
    assert_eq!(fx.snapshot(), before);
    assert_eq!(fs::metadata(&manifest).expect("manifest").ino(), inode);
    fx.put(CHANNEL_MANIFEST, b"tampered", 0o644);
    let tampered = fx.snapshot();
    let requests_before = fx.requests();
    let rejected = release.run_file_by_file(&fx, ToolchainPhase::ReadOnly);
    assert!(
        rejected
            .stderr
            .contains("rust_channel_manifest_digest_mismatch"),
        "{rejected:?}"
    );
    assert_eq!(rejected.requests, requests_before, "{rejected:?}");
    assert_eq!(fx.snapshot(), tampered);
}
