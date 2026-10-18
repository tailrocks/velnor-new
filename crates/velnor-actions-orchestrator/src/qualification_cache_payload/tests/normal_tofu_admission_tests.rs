use super::super::NormalTofuProvidersCachePayloadRequest;
use super::backend::validate_restore_observation;
use super::context::{GitHubRunContext, git_sha, positive_u64, validate_event_repository};
use crate::cache_action_stage::CacheActionCompression;
use crate::qualification_cache_payload::CacheActionObservation;
use std::path::PathBuf;
use tempfile::TempDir;
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation,
};

struct Fixture {
    _temp: TempDir,
    runner_temp: PathBuf,
    workspace: PathBuf,
    cache_key: String,
    run: GitHubRunContext,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = std::fs::canonicalize(temp.path()).expect("canonical temporary directory");
        let runner_temp = root.join("runner-temp");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&runner_temp).expect("runner temp");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let cache_key =
            super::super::cache_key("x86_64-unknown-linux-gnu", "1.13.1", "", &"a".repeat(64))
                .expect("cache key");
        Self {
            _temp: temp,
            runner_temp,
            workspace,
            cache_key,
            run: GitHubRunContext {
                repository: "owner/repo".to_owned(),
                repository_id: 101,
                git_ref: "refs/heads/main".to_owned(),
                source_sha: "a".repeat(40),
                workflow_ref: "owner/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
                workflow_sha: "b".repeat(40),
                run_id: 202,
                run_attempt: 3,
            },
        }
    }

    fn request(&self) -> NormalTofuProvidersCachePayloadRequest<'_> {
        NormalTofuProvidersCachePayloadRequest {
            runner_temp: &self.runner_temp,
            workspace: &self.workspace,
            expected_target: "x86_64-unknown-linux-gnu",
            expected_tofu_version: "1.13.1",
            expected_root: "",
            cache_key: &self.cache_key,
            cache_path: &self.workspace,
            configured_plugin_cache_dir: &self.workspace,
            runtime_observation: "{}",
            previous_stage_claim: None,
            operation_limit_bytes: 4 * 1024 * 1024,
        }
    }

    fn backend_entry(&self, version: String, size_bytes: u64) -> QualificationCacheBackendEntry {
        QualificationCacheBackendEntry {
            id: 303,
            key: self.cache_key.clone(),
            git_ref: self.run.git_ref.clone(),
            version,
            size_bytes,
        }
    }

    fn claim(length: u64) -> String {
        crate::cache_action_stage::StageReceiptClaim::new(1, 2, length, [3; 32]).encode()
    }

    fn runtime(&self) -> super::super::ValidatedTofuRuntime {
        super::super::ValidatedTofuRuntime {
            target: "x86_64-unknown-linux-gnu".to_owned(),
            tofu_version: "1.13.1".to_owned(),
            root: String::new(),
            lockfile_sha256: "a".repeat(64),
            runtime_identity_digest: "b3-".to_owned() + &"c".repeat(64),
            cache_path: self.workspace.clone(),
        }
    }
}

#[test]
fn cold_restore_requires_absent_backend_and_no_tar_outputs() {
    let fixture = Fixture::new();
    let action = CacheActionObservation {
        hit: false,
        matched_key: None,
    };
    let absent = validate_restore_observation(
        &fixture.request(),
        &action,
        None,
        None,
        fixture.run.clone(),
        fixture.runtime(),
        QualificationCacheBackendObservation::Absent,
    )
    .expect("verified cold miss");
    assert!(absent.cache_entry().is_none());
    assert!(absent.compression().is_none());
    assert!(absent.stage_claim().is_none());

    assert_eq!(
        validate_restore_observation(
            &fixture.request(),
            &action,
            None,
            None,
            fixture.run.clone(),
            fixture.runtime(),
            QualificationCacheBackendObservation::Found(
                fixture.backend_entry("c".repeat(64), 1_024)
            ),
        ),
        Err("normal_tofu_cache_backend_action_mismatch")
    );
    assert_eq!(
        validate_restore_observation(
            &fixture.request(),
            &action,
            Some(&Fixture::claim(1_024)),
            Some(CacheActionCompression::Gzip),
            fixture.run.clone(),
            fixture.runtime(),
            QualificationCacheBackendObservation::Absent,
        ),
        Err("normal_tofu_cache_miss_has_stage_evidence")
    );
}

#[test]
fn warm_restore_binds_action_key_compression_version_and_download_size() {
    let fixture = Fixture::new();
    let context = crate::cache_action_stage::StageContext::normal_tofu_providers(
        &fixture.runner_temp,
        &fixture.workspace,
        &fixture.cache_key,
    )
    .expect("stage context");
    let compression = CacheActionCompression::ZstdWithoutLong;
    let version = super::super::super::cache_action_version::version_for_action_compression(
        context.action_path(),
        compression,
    )
    .expect("cache version");
    let action = CacheActionObservation {
        hit: true,
        matched_key: Some(fixture.cache_key.clone()),
    };
    let claim = Fixture::claim(1_024);
    let compressed_archive_size = 1_537;
    let admitted = validate_restore_observation(
        &fixture.request(),
        &action,
        Some(&claim),
        Some(compression),
        fixture.run.clone(),
        fixture.runtime(),
        QualificationCacheBackendObservation::Found(
            fixture.backend_entry(version.clone(), compressed_archive_size),
        ),
    )
    .expect("backend-bound hit");
    let record = admitted.cache_entry().expect("exact backend record");
    assert_eq!(record.id, 303);
    assert_eq!(record.version, version);
    assert_eq!(record.size_bytes, compressed_archive_size);
    assert_eq!(
        admitted.stage_claim().expect("checked claim").length(),
        1_024
    );

    assert_eq!(
        validate_restore_observation(
            &fixture.request(),
            &action,
            Some(&claim),
            Some(CacheActionCompression::Gzip),
            fixture.run.clone(),
            fixture.runtime(),
            QualificationCacheBackendObservation::Found(fixture.backend_entry(version, 1_024),),
        ),
        Err("normal_tofu_cache_backend_entry_mismatch")
    );
    assert_eq!(
        validate_restore_observation(
            &fixture.request(),
            &action,
            Some(&claim),
            Some(compression),
            fixture.run.clone(),
            fixture.runtime(),
            QualificationCacheBackendObservation::Found(
                fixture.backend_entry("d".repeat(64), compressed_archive_size),
            ),
        ),
        Err("normal_tofu_cache_backend_entry_mismatch")
    );
}

#[test]
fn backend_archive_size_has_its_own_bounded_identity() {
    let fixture = Fixture::new();
    let context = crate::cache_action_stage::StageContext::normal_tofu_providers(
        &fixture.runner_temp,
        &fixture.workspace,
        &fixture.cache_key,
    )
    .expect("stage context");
    let compression = CacheActionCompression::Gzip;
    let version = super::super::super::cache_action_version::version_for_action_compression(
        context.action_path(),
        compression,
    )
    .expect("cache version");
    let action = CacheActionObservation {
        hit: true,
        matched_key: Some(fixture.cache_key.clone()),
    };
    let raw_length = 1_024;
    let compressed_length = raw_length + 317;
    assert_ne!(compressed_length, raw_length);
    let accepted = validate_restore_observation(
        &fixture.request(),
        &action,
        Some(&Fixture::claim(raw_length)),
        Some(compression),
        fixture.run.clone(),
        fixture.runtime(),
        QualificationCacheBackendObservation::Found(
            fixture.backend_entry(version.clone(), compressed_length),
        ),
    );
    assert!(accepted.is_ok());

    let limit = super::backend::max_cache_archive_bytes(fixture.request().operation_limit_bytes)
        .expect("bounded archive ceiling");
    assert_eq!(
        validate_restore_observation(
            &fixture.request(),
            &action,
            Some(&Fixture::claim(raw_length)),
            Some(compression),
            fixture.run.clone(),
            fixture.runtime(),
            QualificationCacheBackendObservation::Found(
                fixture.backend_entry(version.clone(), limit + 1),
            ),
        ),
        Err("normal_tofu_cache_backend_entry_mismatch")
    );

    let mut smaller_request = fixture.request();
    smaller_request.operation_limit_bytes = 1_024;
    assert_eq!(
        validate_restore_observation(
            &smaller_request,
            &action,
            Some(&Fixture::claim(raw_length + 1)),
            Some(compression),
            fixture.run.clone(),
            fixture.runtime(),
            QualificationCacheBackendObservation::Found(
                fixture.backend_entry(version, compressed_length),
            ),
        ),
        Err("normal_tofu_cache_backend_entry_mismatch")
    );
}

#[test]
fn forged_stage_claim_does_not_open_the_retained_stage_file()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let root = tempfile::tempdir()?;
    let runner_temp = root.path().join("runner-temp");
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&runner_temp)?;
    std::fs::create_dir(&workspace)?;
    let cache_key =
        super::super::cache_key("x86_64-unknown-linux-gnu", "1.13.1", "", &"a".repeat(64))?;
    let context = crate::cache_action_stage::StageContext::normal_tofu_providers(
        &runner_temp,
        &workspace,
        &cache_key,
    )?;
    let mut writer = crate::cache_action_stage::create_stage(&context)?;
    writer.write_all(b"checked transport")?;
    let claim = writer.finish()?;
    let encoded = claim.encode();
    let (prefix, _) = encoded.rsplit_once('.').expect("claim digest");
    let forged = format!("{prefix}.{}", "f".repeat(64));
    let forged = crate::cache_action_stage::StageReceiptClaim::parse(&forged)?;
    assert!(crate::cache_action_stage::open_stage(&context, &forged).is_err());
    Ok(())
}

#[test]
fn hit_without_complete_action_evidence_fails_closed() {
    let fixture = Fixture::new();
    let action = CacheActionObservation {
        hit: true,
        matched_key: Some(fixture.cache_key.clone()),
    };
    let found =
        QualificationCacheBackendObservation::Found(fixture.backend_entry("c".repeat(64), 1_024));
    assert_eq!(
        validate_restore_observation(
            &fixture.request(),
            &action,
            None,
            Some(CacheActionCompression::Gzip),
            fixture.run.clone(),
            fixture.runtime(),
            found.clone(),
        ),
        Err("normal_tofu_cache_stage_claim_missing")
    );
    assert_eq!(
        validate_restore_observation(
            &fixture.request(),
            &action,
            Some(&Fixture::claim(1_024)),
            None,
            fixture.run.clone(),
            fixture.runtime(),
            found,
        ),
        Err("normal_tofu_cache_compression_missing")
    );
}

#[test]
fn github_event_repository_id_and_slug_must_agree() {
    let event = r#"{"repository":{"full_name":"Owner/Repo","id":101}}"#;
    assert_eq!(validate_event_repository(event, "owner/repo", 101), Ok(()));
    assert_eq!(
        validate_event_repository(event, "other/repo", 101),
        Err("normal_tofu_github_repository_mismatch")
    );
    assert_eq!(
        validate_event_repository(event, "owner/repo", 102),
        Err("normal_tofu_github_repository_mismatch")
    );
}

#[test]
fn runner_numeric_coordinates_and_hashes_are_canonical() {
    assert_eq!(positive_u64("1"), Ok(1));
    for value in ["", "0", "01", "+1", " 1"] {
        assert_eq!(
            positive_u64(value),
            Err("normal_tofu_github_context_invalid")
        );
    }
    let sha = "f".repeat(40);
    assert_eq!(git_sha(&sha).as_deref(), Ok(sha.as_str()));
    for value in ["F".repeat(40), "a".repeat(39), "a".repeat(65)] {
        assert_eq!(git_sha(&value), Err("normal_tofu_github_context_invalid"));
    }
}
