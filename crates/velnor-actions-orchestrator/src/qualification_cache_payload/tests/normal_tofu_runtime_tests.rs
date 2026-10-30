use super::*;

use std::fs;
use std::io::{Read, Seek, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    runner_temp: PathBuf,
    workspace: PathBuf,
    cache_path: PathBuf,
    configured_cache_path: PathBuf,
    lockfile_sha256: String,
    binary_path: PathBuf,
    binary_sha256: String,
    runtime_observation: String,
    cache_key: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = fs::canonicalize(temp.path()).expect("canonical temporary directory");
        let runner_temp = root.join("runner-temp");
        let workspace = root.join("workspace");
        fs::create_dir_all(&runner_temp).expect("runner temp");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::write(workspace.join(".terraform.lock.hcl"), b"provider-lock\n").expect("lockfile");
        let binary_path = root.join("tofu");
        fs::write(&binary_path, b"measured-tofu-binary").expect("binary");
        fs::set_permissions(&binary_path, fs::Permissions::from_mode(0o700))
            .expect("executable mode");
        let lockfile_sha256 = sha256_file(
            &workspace.join(".terraform.lock.hcl"),
            MAX_LOCKFILE_BYTES,
            false,
        )
        .expect("lockfile digest");
        let binary_sha256 =
            sha256_file(&binary_path, MAX_BINARY_BYTES, true).expect("binary digest");
        let cache_path = cache_path(&runner_temp, "").expect("provider cache path");
        let runtime_observation = observation(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "",
            &cache_path,
            &lockfile_sha256,
            &binary_path,
            &binary_sha256,
        );
        let cache_key = cache_key("x86_64-unknown-linux-gnu", "1.13.1", "", &lockfile_sha256)
            .expect("cache key");
        Self {
            _temp: temp,
            root,
            runner_temp,
            workspace,
            configured_cache_path: cache_path.clone(),
            cache_path,
            lockfile_sha256,
            binary_path,
            binary_sha256,
            runtime_observation,
            cache_key,
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
            cache_path: &self.cache_path,
            configured_plugin_cache_dir: &self.configured_cache_path,
            runtime_observation: &self.runtime_observation,
            previous_stage_claim: None,
            operation_limit_bytes: 65_536,
        }
    }
}

fn observation(
    target: &str,
    tofu_version: &str,
    root: &str,
    plugin_cache_dir: &Path,
    lockfile_sha256: &str,
    binary_path: &Path,
    binary_sha256: &str,
) -> String {
    canonical_json_str(&json!({
        "lockfile_sha256": lockfile_sha256,
        "plugin_cache_dir": plugin_cache_dir.to_str().expect("cache path text"),
        "root": root,
        "schema": SCHEMA,
        "target": target,
        "tofu_binary_path": binary_path.to_str().expect("binary path text"),
        "tofu_binary_sha256": binary_sha256,
        "tofu_version": tofu_version,
    }))
    .expect("canonical runtime observation")
}

#[test]
fn runtime_request_binds_actual_lockfile_binary_path_key_and_root() {
    let fixture = Fixture::new();
    let validated = validate_request(&fixture.request()).expect("validated runtime");

    assert_eq!(validated.target, "x86_64-unknown-linux-gnu");
    assert_eq!(validated.tofu_version, "1.13.1");
    assert_eq!(validated.root, "");
    assert_eq!(validated.lockfile_sha256, fixture.lockfile_sha256);
    assert_eq!(validated.cache_path, fixture.cache_path);
    assert!(validated.runtime_identity_digest.starts_with("b3-"));
}

#[test]
fn runtime_request_rejects_old_key_and_mismatched_runtime_facts() {
    let fixture = Fixture::new();
    for (target, version, root, key) in [
        (
            "aarch64-unknown-linux-gnu",
            "1.13.1",
            "",
            fixture.cache_key.as_str(),
        ),
        (
            "x86_64-unknown-linux-gnu",
            "1.13.0",
            "",
            fixture.cache_key.as_str(),
        ),
        (
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "stacks/a",
            fixture.cache_key.as_str(),
        ),
        (
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "",
            "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-old",
        ),
    ] {
        let runtime_observation = observation(
            target,
            version,
            root,
            &fixture.cache_path,
            &fixture.lockfile_sha256,
            &fixture.binary_path,
            &fixture.binary_sha256,
        );
        let mut request = fixture.request();
        request.runtime_observation = &runtime_observation;
        request.cache_key = key;
        assert!(validate_request(&request).is_err());
    }
}

#[test]
fn runtime_request_rejects_changed_lockfile_binary_or_configured_cache_path() {
    let fixture = Fixture::new();
    fs::write(
        fixture.workspace.join(".terraform.lock.hcl"),
        b"changed lock\n",
    )
    .expect("change lockfile");
    assert_eq!(
        validate_request(&fixture.request()),
        Err("normal_tofu_lockfile_digest_mismatch")
    );

    fs::write(
        fixture.workspace.join(".terraform.lock.hcl"),
        b"provider-lock\n",
    )
    .expect("restore lockfile");
    fs::write(&fixture.binary_path, b"changed binary").expect("change binary");
    assert_eq!(
        validate_request(&fixture.request()),
        Err("normal_tofu_binary_digest_mismatch")
    );

    fs::write(&fixture.binary_path, b"measured-tofu-binary").expect("restore binary");
    let other = fixture.root.join("other-cache");
    let mut request = fixture.request();
    request.configured_plugin_cache_dir = &other;
    assert_eq!(
        validate_request(&request),
        Err("normal_tofu_cache_path_mismatch")
    );
}

#[test]
fn runtime_request_requires_closed_canonical_observation_and_operation_limit() {
    let fixture = Fixture::new();
    for observation in ["{}", "{\"schema\":\"velnor.tofu-cache-runtime/v1\"}"] {
        let mut request = fixture.request();
        request.runtime_observation = observation;
        assert!(validate_request(&request).is_err());
    }
    let noncanonical = format!(" {}", fixture.runtime_observation);
    let mut request = fixture.request();
    request.runtime_observation = &noncanonical;
    assert_eq!(
        validate_request(&request),
        Err("normal_tofu_runtime_noncanonical")
    );

    let mut request = fixture.request();
    request.operation_limit_bytes = 0;
    assert_eq!(
        validate_request(&request),
        Err("normal_tofu_operation_limit_invalid")
    );
}

#[path = "normal_tofu_export_tests.rs"]
mod export_tests;
