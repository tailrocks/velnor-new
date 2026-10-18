use super::super::{
    NormalTofuProvidersCachePayloadRequest, cache_key, cache_path,
    export_normal_tofu_providers_cache_payload,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use velnor_actions_contract::canonical_json_str;

#[test]
fn normal_tofu_export_rejects_a_writable_source_ancestor_before_stage_creation() {
    let scratch = tempfile::tempdir().expect("scratch");
    let root = fs::canonicalize(scratch.path()).expect("canonical root");
    let runner_temp = root.join("runner-temp");
    let workspace = root.join("workspace");
    fs::create_dir_all(&runner_temp).expect("runner temp");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join(".terraform.lock.hcl"), b"provider-lock\n").expect("lockfile");
    let lockfile_sha256 = hex(&Sha256::digest(b"provider-lock\n"));
    let binary = root.join("tofu");
    fs::write(&binary, b"measured-tofu-binary").expect("binary");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).expect("binary mode");
    let binary_sha256 = hex(&Sha256::digest(b"measured-tofu-binary"));
    let plugin_cache_dir = cache_path(&runner_temp, "").expect("cache path");
    fs::create_dir_all(&plugin_cache_dir).expect("plugin cache");
    fs::write(plugin_cache_dir.join("provider.zip"), b"provider bytes").expect("provider file");
    let key =
        cache_key("x86_64-unknown-linux-gnu", "1.13.1", "", &lockfile_sha256).expect("cache key");
    let observation = canonical_json_str(&json!({
        "lockfile_sha256": lockfile_sha256,
        "plugin_cache_dir": plugin_cache_dir.to_str().expect("cache path text"),
        "root": "",
        "schema": "velnor.tofu-cache-runtime/v1",
        "target": "x86_64-unknown-linux-gnu",
        "tofu_binary_path": binary.to_str().expect("binary path text"),
        "tofu_binary_sha256": binary_sha256,
        "tofu_version": "1.13.1",
    }))
    .expect("canonical observation");
    let request = NormalTofuProvidersCachePayloadRequest {
        runner_temp: &runner_temp,
        workspace: &workspace,
        expected_target: "x86_64-unknown-linux-gnu",
        expected_tofu_version: "1.13.1",
        expected_root: "",
        cache_key: &key,
        cache_path: &plugin_cache_dir,
        configured_plugin_cache_dir: &plugin_cache_dir,
        runtime_observation: &observation,
        previous_stage_claim: None,
        operation_limit_bytes: 65_536,
    };
    let unsafe_parent = plugin_cache_dir.parent().expect("cache parent");
    fs::set_permissions(unsafe_parent, fs::Permissions::from_mode(0o777))
        .expect("writable source ancestor");
    let stage_context = crate::cache_action_stage::StageContext::normal_tofu_providers(
        &runner_temp,
        &workspace,
        &key,
    )
    .expect("action stage context");

    assert_eq!(
        export_normal_tofu_providers_cache_payload(&request).map(|_| ()),
        Err("qualification_cache_payload_source_ancestor_invalid")
    );
    assert!(!stage_context.action_path().exists());
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}
