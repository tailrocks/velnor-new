// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super as parent;
use super::{preview_consumer_intent, preview_fixture};

#[test]
fn qualification_intent_matches_rendered_intent_without_enabling_publishing()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    preview_fixture(temp.path())?;
    let mut prep = crate::prepare::prepare_for_candidate(temp.path())?;
    let git = crate::crate_release_git::ReleaseGit::host_fixture();
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(temp.path())
        .output()?;
    let sha = std::str::from_utf8(&sha.stdout)?.trim();
    let source = crate::crate_release_qualification_snapshot::snapshot_approved_source(
        temp.path(),
        sha,
        &git,
    )?;
    let raw = parent::release_metadata_json(&prep, "Cargo.toml")?;
    let raw = raw.replace(
        temp.path().canonicalize()?.to_str().ok_or("path")?,
        "/source",
    );
    let metadata =
        crate::crate_release_qualification_intent::remap_metadata(raw.as_bytes(), source.root())?;
    let manifest = std::fs::read(temp.path().join(".velnor/release-manifest.json"))?;
    let metadata_config = qualification_metadata(prep.catalog.version(velnor_actions_mise::PinnedTool::Rust));
    let derive = |config: &velnor_actions_contract::VelnorConfig| {
        parent::derive_intent_from_snapshot(
            &source,
            config,
            &metadata,
            &manifest,
            &metadata_config,
            "example/demo",
            "main",
        )
    };
    let actual = derive(&prep.config)?;
    let rendered = preview_consumer_intent(temp.path())?;
    let expected = crate::crate_release_runtime::parse_crate_release_intent(rendered.as_bytes())?;
    assert_eq!(toml::to_string(&actual)?, toml::to_string(&expected)?);
    assert!(parent::release_files(&prep).is_err());
    check_forged_metadata(&source, &prep.config, &raw, &manifest, &metadata_config)?;
    prep.config
        .stacks
        .rust
        .as_mut()
        .ok_or("missing Rust")?
        .release
        .trusted_reviewer_id = 456;
    let changed = derive(&prep.config)?;
    assert_ne!(actual.trusted_reviewer_id, changed.trusted_reviewer_id);
    check_recovery(&mut prep.config, sha, &derive)?;
    assert!(
        crate::crate_release_qualification_snapshot::snapshot_approved_source(
            temp.path(),
            &"0".repeat(40),
            &git
        )
        .is_err()
    );
    Ok(())
}

pub(super) fn qualification_metadata(rust_version: &str) -> velnor_actions_contract::config::ReleaseRehearsalMetadataConfig {
    velnor_actions_contract::config::ReleaseRehearsalMetadataConfig {
        manifest_path: "Cargo.toml".into(),
        intent_config_path: ".velnor/config.toml".into(),
        qualification: velnor_actions_contract::config::ReleaseQualificationPolicy {
            candidate_repository: "candidate/repo".into(),
            candidate_repository_id: 1,
            candidate_default_branch: "main".into(),
            candidate_required_workflow_path: ".github/workflows/ci.yml".into(),
            review_proof: velnor_actions_contract::config::ReleaseReviewProof::CompletionCheck {
                completion_app_id: 1,
                completion_check_name: "complete".into(),
                reviewer_app_ids: [1].into_iter().collect(),
            },
            historical_manifest: velnor_actions_contract::config::ReleaseHistoricalManifestPolicy {
                version: "0.1.5".into(),
                source_sha: "b".repeat(40),
                sha256: "c".repeat(64),
            },
        },
        rust_version: rust_version.to_owned(),
    }
}

fn check_forged_metadata(
    source: &crate::crate_release_qualification_snapshot::SourceSnapshot,
    config: &velnor_actions_contract::VelnorConfig,
    raw: &str,
    manifest: &[u8],
    metadata_config: &velnor_actions_contract::config::ReleaseRehearsalMetadataConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut forged: serde_json::Value = serde_json::from_str(raw)?;
    forged["packages"][0]["manifest_path"] = "/source/missing/Cargo.toml".into();
    let forged = crate::crate_release_qualification_intent::remap_metadata(
        serde_json::to_string(&forged)?.as_bytes(),
        source.root(),
    )?;
    assert!(
        parent::derive_intent_from_snapshot(
            source,
            config,
            &forged,
            manifest,
            metadata_config,
            "example/demo",
            "main"
        )
        .is_err()
    );
    assert!(
        crate::crate_release_qualification_intent::remap_metadata(
            raw.replace("/source/Cargo.toml", "/source/../Cargo.toml")
                .as_bytes(),
            source.root()
        )
        .is_err()
    );
    assert!(
        crate::crate_release_qualification_intent::remap_metadata(
            raw.replace("/source", "/source-other").as_bytes(),
            source.root()
        )
        .is_err()
    );
    Ok(())
}

fn check_recovery(
    config: &mut velnor_actions_contract::VelnorConfig,
    sha: &str,
    derive: &impl Fn(
        &velnor_actions_contract::VelnorConfig,
    ) -> Result<
        velnor_actions_contract::config::CrateReleaseIntent,
        crate::OrchestratorError,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    let release = &mut config.stacks.rust.as_mut().ok_or("missing Rust")?.release;
    release.recovery = Some(velnor_actions_contract::config::RecoveryRelease {
        package: "demo".into(),
        version: "0.1.0".into(),
        source_sha: sha.to_owned(),
    });
    let recovery = derive(config)?;
    assert_eq!(recovery.recovery.ok_or("missing recovery")?.source_sha, sha);
    config
        .stacks
        .rust
        .as_mut()
        .ok_or("missing Rust")?
        .release
        .recovery
        .as_mut()
        .ok_or("missing recovery")?
        .source_sha = "0".repeat(40);
    assert!(derive(config).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_snapshot_never_leaves_its_verified_tree() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir()?;
    preview_fixture(temp.path())?;
    let link = temp.path().join("src/link.rs");
    symlink("lib.rs", &link)?;
    for args in [
        &["add", "."][..],
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "link",
        ][..],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(temp.path())
                .status()?
                .success()
        );
    }
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(temp.path())
        .output()?;
    let sha = std::str::from_utf8(&output.stdout)?.trim();
    let snapshot = crate::crate_release_qualification_snapshot::snapshot_approved_source(
        temp.path(),
        sha,
        &crate::crate_release_git::ReleaseGit::host_fixture(),
    )?;
    assert_eq!(
        std::fs::read(snapshot.root().join("src/link.rs"))?,
        std::fs::read(temp.path().join("src/lib.rs"))?
    );
    Ok(())
}
