use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    CacheKey, CachedRelease, NativeArtifactProvider, cached_release, image_binding_fingerprint,
    validate_fetched_release,
};
use crate::artifact_admission::{github, manifest};
use crate::artifact_admission::{hash, release::VerifiedRelease};
use crate::error::HostError;

fn release(source: &str, authority: &str, archive_sha: &str) -> VerifiedRelease {
    let manifest = crate::artifact_admission::manifest::ProbeManifest {
        source_commit: source.to_owned(),
        workflow_authority_sha: authority.to_owned(),
        platform: crate::artifact_admission::manifest::PLATFORM.to_owned(),
        archive_format: crate::artifact_admission::manifest::ARCHIVE_FORMAT.to_owned(),
        oci_index_sha256: "b".repeat(64),
        image_manifest_digest: format!("sha256:{}", "c".repeat(64)),
        config_digest: format!("sha256:{}", "d".repeat(64)),
        archive_sha256: archive_sha.to_owned(),
    };
    VerifiedRelease {
        source_sha: source.to_owned(),
        authority_sha: authority.to_owned(),
        archive_sha: archive_sha.to_owned(),
        archive: Vec::new(),
        manifest_bytes: Vec::new(),
        checksum_bytes: Vec::new(),
        runner_asset: github::Asset {
            id: 1,
            name: manifest::RUNNER_ASSET.to_owned(),
            size: 1,
            digest: format!("sha256:{}", "e".repeat(64)),
        },
        dind_asset: github::Asset {
            id: 2,
            name: manifest::DIND_ASSET.to_owned(),
            size: 1,
            digest: format!("sha256:{}", "f".repeat(64)),
        },
        manifest,
        archive_identity: crate::artifact_admission::archive::ArchiveIdentity {
            image_manifest_digest: format!("sha256:{}", "c".repeat(64)),
            image_manifest_media_type: "application/vnd.oci.image.manifest.v1+json".to_owned(),
            image_manifest_size: 0,
            config_digest: format!("sha256:{}", "d".repeat(64)),
            config: bollard::models::ContainerConfig::default(),
        },
    }
}

#[test]
fn provider_factory_uses_only_the_current_compiled_pin_pair() -> Result<(), HostError> {
    let Some(identity) = crate::compile_identity::compiled_release_identity() else {
        assert_eq!(
            NativeArtifactProvider::from_compiled_release_identity(
                "ghp_test-token",
                Path::new("/tmp/velnor-state"),
            )
            .map(|_| ()),
            Err(HostError::Identity)
        );
        return Ok(());
    };
    let state_directory = Path::new("/tmp/velnor-state");
    let provider =
        NativeArtifactProvider::from_compiled_release_identity("ghp_test-token", state_directory)?;
    assert_eq!(provider.identity, identity);
    assert_eq!(
        provider.key.source_sha,
        hash::lowercase_hex(identity.source_sha())
    );
    assert_eq!(provider.key.helper_sha256, *identity.helper_sha256());
    assert_eq!(provider.pat, "ghp_test-token");
    assert_eq!(provider.state_directory, state_directory);
    for invalid in ["", "token\nvalue", &"x".repeat(4097)] {
        assert_eq!(
            NativeArtifactProvider::from_compiled_release_identity(invalid, state_directory)
                .map(|_| ()),
            Err(HostError::Identity)
        );
    }
    Ok(())
}

#[test]
fn cache_requires_source_helper_and_state_directory() {
    let key = CacheKey {
        source_sha: "a".repeat(40),
        helper_sha256: [0x22; 32],
        state_directory: PathBuf::from("/private/state-a"),
    };
    let cached = CachedRelease {
        key: key.clone(),
        release: Arc::new(release(&key.source_sha, &key.source_sha, &"b".repeat(64))),
    };
    assert!(cached_release(Some(&cached), &key).is_some());
    let mut wrong_source = key.clone();
    wrong_source.source_sha = "c".repeat(40);
    let mut wrong_helper = key.clone();
    wrong_helper.helper_sha256[0] ^= 1;
    let mut wrong_state = key.clone();
    wrong_state.state_directory = PathBuf::from("/private/state-b");
    assert!(cached_release(Some(&cached), &wrong_source).is_none());
    assert!(cached_release(Some(&cached), &wrong_helper).is_none());
    assert!(cached_release(Some(&cached), &wrong_state).is_none());
}

#[test]
fn fetched_release_must_match_the_compiled_source_and_manifest_archive() {
    let key = CacheKey {
        source_sha: "a".repeat(40),
        helper_sha256: [0x22; 32],
        state_directory: PathBuf::from("/private/state-a"),
    };
    let valid = release(&key.source_sha, &key.source_sha, &"b".repeat(64));
    assert_eq!(validate_fetched_release(&key, &valid), Ok(()));
    assert_eq!(
        validate_fetched_release(
            &key,
            &release(&"c".repeat(40), &key.source_sha, &"b".repeat(64))
        ),
        Err(HostError::Identity)
    );
    assert_eq!(
        validate_fetched_release(
            &key,
            &release(&key.source_sha, &"c".repeat(40), &"b".repeat(64))
        ),
        Err(HostError::Identity)
    );
    let mut wrong_archive = release(&key.source_sha, &key.source_sha, &"b".repeat(64));
    wrong_archive.archive_sha = "c".repeat(64);
    assert_eq!(
        validate_fetched_release(&key, &wrong_archive),
        Err(HostError::Identity)
    );
}

#[test]
fn fingerprint_binds_archive_engine_root_and_runtime_id() -> Result<(), HostError> {
    let release = release(&"a".repeat(40), &"a".repeat(40), &"e".repeat(64));
    let expected = image_binding_fingerprint(&release, "engine", "/data", "sha256:runtime");
    if expected == image_binding_fingerprint(&release, "other", "/data", "sha256:runtime")
        || expected == image_binding_fingerprint(&release, "engine", "/other", "sha256:runtime")
        || expected == image_binding_fingerprint(&release, "engine", "/data", "sha256:other")
    {
        return Err(HostError::Identity);
    }
    Ok(())
}
