// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::Path, process::Command};

pub(super) fn hash(bytes: &[u8]) -> String {
    super::support::hex(&Sha256::digest(bytes))
}

fn image(role: &str, source: &str) -> Value {
    json!({"repository":format!("ghcr.io/tailrocks/velnor-{role}"),"tag":format!("sha-{source}"),"registry_digest":format!("sha256:{}", "a".repeat(64)),"platform_manifest_digest":format!("sha256:{}", "a".repeat(64)),"config_digest":format!("sha256:{}", "b".repeat(64)),"build_source":{"metadata_sha256":"c".repeat(64),"build_type":"buildkit","source_repository":"https://github.com/tailrocks/velnor-new","source_commit":source,"context_path":"images/x","dockerfile_path":"images/x/Dockerfile","builder_platform":"linux/amd64","named_contexts":{}}})
}

pub(super) fn invoke(dir: &Path) -> std::io::Result<std::process::Output> {
    Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
        .args(["image-maintenance", "assemble-mac-attestation"])
        .args([
            "--context",
            "context.json",
            "--receipt",
            "receipt.json",
            "--manifest",
            "manifest.json",
            "--runner-lock",
            "runner.lock",
            "--generator-lock",
            "generator.lock",
            "--sdk-evidence",
            "sdk.json",
            "--output",
            "output.json",
        ])
        .current_dir(dir)
        .output()
}

pub(super) struct Fixture {
    pub(super) dir: tempfile::TempDir,
    pub(super) context: Value,
    receipt: Value,
    receipt_bytes: Vec<u8>,
}

pub(super) fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sdk_bytes = fs::read(root.join("fixtures/consumer-release-manifest.json"))?;
    let sdk: Value = serde_json::from_slice(&sdk_bytes)?;
    let mut lock: toml::Value =
        toml::from_str(&fs::read_to_string(root.join(".velnor/generator.lock"))?)?;
    lock["generator"]["version"] =
        toml::Value::String(sdk["version"].as_str().ok_or("version")?.into());
    lock["generator"]["commit"] =
        toml::Value::String(sdk["commit"].as_str().ok_or("commit")?.into());
    for item in lock["generator"]["binaries"]
        .as_array_mut()
        .ok_or("binaries")?
    {
        let target = item["target"].as_str().ok_or("target")?;
        let found = sdk["targets"]
            .as_array()
            .ok_or("targets")?
            .iter()
            .find(|v| v["target"] == target)
            .ok_or("sdk target")?;
        item["artifact"] =
            toml::Value::String(found["artifact"].as_str().ok_or("artifact")?.into());
        item["sha256"] = toml::Value::String(found["sha256"].as_str().ok_or("sha")?.into());
    }
    let runner_lock = b"runner_version = \"2.338.0\"\ndocker_version = \"29.9.1\"\nbuildx_version = \"0.38.0\"\ncompose_version = \"5.6.0\"\nnode_version = \"26.12.0\"\n";
    let source = "d".repeat(40);
    let versions = json!({"runner":"2.338.0","docker":"29.9.1","buildx":"0.38.0","compose":"5.6.0","node":"26.12.0"});
    let images = json!({"runner":image("runner", &source),"dind":image("dind", &source),"resource_probe":image("resource-probe", &source)});
    let manifest = json!({"schema_version":2,"repository":"tailrocks/velnor-new","source_ref":"refs/heads/main","source_commit":source,"runner_lock_sha256":hash(runner_lock),"signer_workflow":".github/workflows/product-release-images.yml","signer_ref":"refs/heads/main","workflow_authority_sha":source,"trusted_signing_identity":{"oidc_issuer":"https://token.actions.githubusercontent.com","certificate_identity":"identity"},"platform":"linux/amd64","versions":versions,"images":images,"resource_probe":{"protocol_version":1,"image_user":"65532:65532","entrypoint":"/velnor/resource-probe","working_directory":"/"}});
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let receipt_image = |role: &str| {
        let v = &manifest["images"][role];
        json!({"repository":v["repository"],"registry_digest":v["registry_digest"],"platform_manifest_digest":v["platform_manifest_digest"],"config_digest":v["config_digest"],"local_id":v["config_digest"]})
    };
    let receipt = json!({"schema":"velnor.runner-image-mac-qualification.v1","source_commit":source,"run_id":7,"run_attempt":1,"publisher_artifact_id":9,"manifest_sha256":hash(&manifest_bytes),"runner_lock_sha256":hash(runner_lock),"platform":"linux/amd64","images":{"runner":receipt_image("runner"),"dind":receipt_image("dind"),"resource_probe":receipt_image("resource_probe")},"outer_daemon":{"id":"outer","root":"/var/lib/docker","store":"classic"},"inner_daemon_id":"inner","versions":versions,"cleanup_complete":true});
    let receipt_bytes = serde_json::to_vec(&receipt)?;
    let context = json!({"public_authority":{"repository":"tailrocks/velnor-new","repository_id":1_390_620_900,"source_commit":source,"release_run_id":7,"release_run_attempt":1,"producer_artifact_id":8,"publisher_artifact_id":9},"companion":{"repository":"donbeave/velnor-mac-qualification","repository_id":1_451_604_567,"owner_id":139_017,"workflow_path":".github/workflows/qualify-runner-family.yml","workflow_ref":"refs/heads/main","workflow_sha":"e".repeat(40),"run_id":10,"run_attempt":1,"qualification_job_id":11,"runner_id":2,"receipt_artifact_id":12,"receipt_artifact_digest":format!("sha256:{}", "f".repeat(64))},"sdk":{"repository":"tailrocks/velnor-new","version":sdk["version"],"source_commit":sdk["commit"],"authority":{"channel":"stable","release_manifest_sha256":hash(&sdk_bytes)},"target":"aarch64-apple-darwin","asset_sha256":"a".repeat(64)}});
    for (name, data) in [
        ("context.json", serde_json::to_vec(&context)?),
        ("receipt.json", receipt_bytes.clone()),
        ("manifest.json", manifest_bytes),
        ("runner.lock", runner_lock.to_vec()),
        ("generator.lock", toml::to_string(&lock)?.into_bytes()),
        ("sdk.json", sdk_bytes),
    ] {
        fs::write(dir.path().join(name), data)?;
    }
    Ok(Fixture {
        dir,
        context,
        receipt,
        receipt_bytes,
    })
}

#[test]
fn unsigned_mac_assembly_binds_exact_inputs_and_never_clobbers() -> Result<(), Box<dyn Error>> {
    let Fixture {
        dir,
        context,
        receipt,
        receipt_bytes,
    } = fixture()?;
    let result = invoke(dir.path())?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = fs::read(dir.path().join("output.json"))?;
    let parsed: Value = serde_json::from_slice(&output)?;
    assert_eq!(parsed["companion"]["repository_id"], 1_451_604_567_u64);
    assert_eq!(parsed["companion"]["runner_id"], 2_u64);
    assert_eq!(
        STANDARD.decode(
            parsed["qualification"]["receipt_base64"]
                .as_str()
                .ok_or("base64")?
        )?,
        receipt_bytes
    );
    assert_eq!(
        parsed["qualification"]["native_receipt_sha256"],
        hash(&receipt_bytes)
    );
    assert!(!invoke(dir.path())?.status.success());
    assert_eq!(fs::read(dir.path().join("output.json"))?, output);
    fs::remove_file(dir.path().join("output.json"))?;
    reject_invalid_inputs(dir.path(), &context, &receipt)?;
    verify_store_bindings(dir.path(), &receipt)?;
    fs::write(dir.path().join("receipt.json"), &receipt_bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        fs::write(dir.path().join("sentinel"), b"untouched")?;
        symlink("sentinel", dir.path().join("output.json"))?;
        assert!(!invoke(dir.path())?.status.success());
        assert_eq!(fs::read(dir.path().join("sentinel"))?, b"untouched");
        fs::remove_file(dir.path().join("output.json"))?;
        assert!(invoke(dir.path())?.status.success());
        assert_eq!(
            fs::metadata(dir.path().join("output.json"))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    Ok(())
}

fn reject_invalid_inputs(
    dir: &Path,
    context: &Value,
    receipt: &Value,
) -> Result<(), Box<dyn Error>> {
    let mut bad = context.clone();
    bad["companion"]["repository_id"] = json!(1_450_917_587_u64);
    fs::write(dir.join("context.json"), serde_json::to_vec(&bad)?)?;
    assert!(!invoke(dir)?.status.success());
    assert!(!dir.join("output.json").exists());
    assert_eq!(
        fs::read(dir.join("receipt.json"))?,
        serde_json::to_vec(receipt)?
    );
    let mut bad = context.clone();
    bad["public_authority"]["release_run_id"] = json!(999);
    fs::write(dir.join("context.json"), serde_json::to_vec(&bad)?)?;
    assert!(!invoke(dir)?.status.success());
    assert!(!dir.join("output.json").exists());
    let context_bytes = serde_json::to_vec(&context)?;
    let duplicate = format!(
        "{{\"sdk\":{{}},{}",
        &String::from_utf8(context_bytes.clone())?[1..]
    );
    let mut unknown = context.clone();
    unknown["unexpected"] = json!(true);
    for bad in [duplicate.into_bytes(), serde_json::to_vec(&unknown)?] {
        fs::write(dir.join("context.json"), bad)?;
        assert!(!invoke(dir)?.status.success());
        assert!(!dir.join("output.json").exists());
    }
    fs::write(dir.join("context.json"), &context_bytes)?;
    let mut wrong_receipt = receipt.clone();
    wrong_receipt["manifest_sha256"] = json!("f".repeat(64));
    for bad in [b"{}".to_vec(), serde_json::to_vec(&wrong_receipt)?] {
        fs::write(dir.join("receipt.json"), bad)?;
        assert!(!invoke(dir)?.status.success());
        assert!(!dir.join("output.json").exists());
    }
    Ok(())
}

fn verify_store_bindings(dir: &Path, receipt: &Value) -> Result<(), Box<dyn Error>> {
    let mut containerd = receipt.clone();
    containerd["outer_daemon"]["store"] = json!("containerd");
    for role in ["runner", "dind", "resource_probe"] {
        containerd["images"][role]["local_id"] =
            containerd["images"][role]["registry_digest"].clone();
    }
    fs::write(dir.join("receipt.json"), serde_json::to_vec(&containerd)?)?;
    assert!(
        invoke(dir)?.status.success(),
        "containerd registry IDs rejected"
    );
    fs::remove_file(dir.join("output.json"))?;
    for store in ["classic", "containerd", "unknown"] {
        let mut bad = if store == "classic" {
            receipt.clone()
        } else {
            containerd.clone()
        };
        bad["outer_daemon"]["store"] = json!(store);
        if store == "classic" {
            bad["images"]["runner"]["local_id"] =
                bad["images"]["runner"]["registry_digest"].clone();
        }
        if store == "containerd" {
            bad["images"]["runner"]["local_id"] = bad["images"]["runner"]["config_digest"].clone();
        }
        fs::write(dir.join("receipt.json"), serde_json::to_vec(&bad)?)?;
        assert!(
            !invoke(dir)?.status.success(),
            "invalid {store} local identity accepted"
        );
        assert!(!dir.join("output.json").exists());
    }
    Ok(())
}
