use super::super::super::NormalTofuProvidersCachePayloadRequest;
use super::super::context::GitHubRunContext;
use super::preflight_found_entry;
use crate::cache_action_stage::{CacheActionCompression, StageContext};
use crate::qualification_cache_payload::storage_admission::{
    CacheTreeUsage, FreshCapacity, test_budget,
};
use std::path::PathBuf;
use tempfile::TempDir;
use velnor_actions_contract::workflow::QualificationCacheBackendEntry;

struct Fixture {
    _temporary: TempDir,
    runner_temp: PathBuf,
    workspace: PathBuf,
    cache_key: String,
    run: GitHubRunContext,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temporary root");
        let root = std::fs::canonicalize(temporary.path()).expect("canonical temporary root");
        let runner_temp = root.join("runner-temp");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&runner_temp).expect("runner temp");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let cache_key = super::super::super::cache_key(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "",
            &"a".repeat(64),
        )
        .expect("cache key");
        Self {
            _temporary: temporary,
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

    fn entry(&self, version: String, size_bytes: u64) -> QualificationCacheBackendEntry {
        QualificationCacheBackendEntry {
            id: 303,
            key: self.cache_key.clone(),
            git_ref: self.run.git_ref.clone(),
            version,
            size_bytes,
        }
    }

    fn fresh_capacity() -> FreshCapacity {
        FreshCapacity {
            available_bytes: 100_000_000,
            available_inodes: Some(100_000),
            fragment_bytes: 4_096,
        }
    }
}

#[test]
fn preflight_binds_compressed_size_and_exact_action_version_before_restore() {
    let fixture = Fixture::new();
    let compression = CacheActionCompression::ZstdWithoutLong;
    let context = StageContext::normal_tofu_providers(
        &fixture.runner_temp,
        &fixture.workspace,
        &fixture.cache_key,
    )
    .expect("stage context");
    let version =
        crate::qualification_cache_payload::cache_action_version::version_for_action_compression(
            context.action_path(),
            compression,
        )
        .expect("action version");
    let preflight = preflight_found_entry(
        &fixture.request(),
        &fixture.run,
        fixture.entry(version, 1_537),
        &test_budget(),
        CacheTreeUsage::default(),
        CacheTreeUsage::default(),
        Fixture::fresh_capacity(),
    )
    .expect("bounded preflight");

    assert_eq!(preflight.cache_key(), fixture.cache_key);
    assert_eq!(preflight.repository(), "owner/repo");
    assert_eq!(preflight.git_ref(), "refs/heads/main");
    assert_eq!(preflight.compression(), Some(compression));
    assert_eq!(
        preflight.cache_entry().map(|entry| entry.size_bytes),
        Some(1_537)
    );
    assert_eq!(preflight.max_download_bytes(), Some(4_988_634));
    assert!(preflight.max_raw_stage_bytes().expect("raw cap") < 4 * 1024 * 1024);
}

#[test]
fn preflight_rejects_wrong_ref_version_and_archive_over_projection_limit() {
    let fixture = Fixture::new();
    let compression = CacheActionCompression::Gzip;
    let context = StageContext::normal_tofu_providers(
        &fixture.runner_temp,
        &fixture.workspace,
        &fixture.cache_key,
    )
    .expect("stage context");
    let version =
        crate::qualification_cache_payload::cache_action_version::version_for_action_compression(
            context.action_path(),
            compression,
        )
        .expect("action version");
    let mut wrong_ref = fixture.entry(version.clone(), 1_537);
    wrong_ref.git_ref = "refs/heads/other".to_owned();
    assert_eq!(
        preflight_found_entry(
            &fixture.request(),
            &fixture.run,
            wrong_ref,
            &test_budget(),
            CacheTreeUsage::default(),
            CacheTreeUsage::default(),
            Fixture::fresh_capacity(),
        ),
        Err("normal_tofu_cache_backend_entry_mismatch")
    );
    assert_eq!(
        preflight_found_entry(
            &fixture.request(),
            &fixture.run,
            fixture.entry("d".repeat(64), 1_537),
            &test_budget(),
            CacheTreeUsage::default(),
            CacheTreeUsage::default(),
            Fixture::fresh_capacity(),
        ),
        Err("normal_tofu_cache_backend_entry_mismatch")
    );
    assert_eq!(
        preflight_found_entry(
            &fixture.request(),
            &fixture.run,
            fixture.entry(version, 6_000_000),
            &test_budget(),
            CacheTreeUsage::default(),
            CacheTreeUsage::default(),
            Fixture::fresh_capacity(),
        ),
        Err("cache_storage_download_budget_exceeded")
    );
}
