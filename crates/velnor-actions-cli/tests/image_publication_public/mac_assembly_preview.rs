// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mac_assembly::{fixture, hash, invoke};
use serde_json::{Value, json};
use std::{error::Error, fs};

fn record() -> Result<(toml::Table, Vec<u8>), Box<dyn Error>> {
    let source = "e".repeat(40);
    let tag = format!("sdk-preview-{source}");
    let run = |prefix: &str, path: &str, id: u64| {
        json!({
            "workflow_path":path,"workflow_ref":format!("tailrocks/velnor-new/{path}@{}", if prefix == "source" || prefix == "draft" || prefix == "signer" { format!("refs/tags/{tag}") } else { "refs/heads/main".into() }),
            "workflow_sha":source,"run_id":id,"run_attempt":1
        })
    };
    let target = |triple: &str, original, retained, job| {
        json!({
            "producer":{"producer_run_id":4,"producer_run_attempt":1,"payload_artifact_id":original,"payload_archive_sha256":"f".repeat(64)},
            "candidate_payload_artifact_id":retained,"qualification_job_id":job,
            "payload":{"target":triple,"name":format!("velnor-actions-{triple}"),"size":10,"sha256":"a".repeat(64)}
        })
    };
    let workflow = ".github/workflows/product-release-generator.yml";
    let receipt = json!({"schema":3,"repository":"tailrocks/velnor-new","version":"0.1.78","source_sha":source,"tag":tag,"release_id":1,
        "source":run("source",workflow,1),"draft":run("draft",workflow,1),"signer":run("signer",workflow,1),
        "ci":run("ci",".github/workflows/ci.yml",2),"qualification":run("qualification",".github/workflows/source-qualification.yml",3),
        "candidate_source_artifact_id":5,"targets":{"linux":target("x86_64-unknown-linux-gnu",6,7,8),"darwin":target("aarch64-apple-darwin",9,10,11)}});
    let bytes = velnor_actions_contract::canonical_json_bytes(&receipt)?;
    let mut flat = toml::Table::new();
    for key in [
        "schema",
        "repository",
        "version",
        "source_sha",
        "tag",
        "release_id",
        "candidate_source_artifact_id",
    ] {
        insert(&mut flat, key, &receipt[key])?;
    }
    for role in ["source", "draft", "signer", "ci", "qualification"] {
        for key in [
            "workflow_path",
            "workflow_ref",
            "workflow_sha",
            "run_id",
            "run_attempt",
        ] {
            insert(&mut flat, &format!("{role}_{key}"), &receipt[role][key])?;
        }
    }
    for role in ["linux", "darwin"] {
        let item = &receipt["targets"][role];
        for key in [
            "producer_run_id",
            "producer_run_attempt",
            "payload_artifact_id",
            "payload_archive_sha256",
        ] {
            let field = if key.starts_with("producer_") {
                key.to_owned()
            } else {
                format!("producer_{key}")
            };
            insert(
                &mut flat,
                &format!("{role}_{field}"),
                &item["producer"][key],
            )?;
        }
        for key in ["candidate_payload_artifact_id", "qualification_job_id"] {
            insert(&mut flat, &format!("{role}_{key}"), &item[key])?;
        }
        for key in ["target", "name", "size", "sha256"] {
            insert(
                &mut flat,
                &format!("{role}_payload_{key}"),
                &item["payload"][key],
            )?;
        }
    }
    flat.insert("receipt_sha256".into(), toml::Value::String(hash(&bytes)));
    flat.insert("bundle_sha256".into(), toml::Value::String("b".repeat(64)));
    flat.insert("publisher_run_attempt".into(), toml::Value::Integer(1));
    flat.insert("publisher_job_id".into(), toml::Value::Integer(12));
    Ok((flat, bytes))
}

fn insert(table: &mut toml::Table, key: &str, value: &Value) -> Result<(), Box<dyn Error>> {
    let value = if let Some(value) = value.as_str() {
        toml::Value::String(value.into())
    } else {
        toml::Value::Integer(i64::try_from(value.as_u64().ok_or("number")?)?)
    };
    table.insert(key.into(), value);
    Ok(())
}

#[test]
fn unsigned_assembly_requires_explicit_matching_preview_runtime() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let root = fixture.dir.path();
    let (record, bytes) = record()?;
    let mut lock: toml::Table = toml::from_str(&fs::read_to_string(root.join("generator.lock"))?)?;
    lock.insert("runtime-preview".into(), toml::Value::Table(record.clone()));
    fs::write(root.join("generator.lock"), toml::to_string(&lock)?)?;
    fs::write(root.join("sdk.json"), &bytes)?;
    let mut context = fixture.context;
    context["sdk"]["version"] = json!("0.1.78");
    context["sdk"]["source_commit"] = json!("e".repeat(40));
    context["sdk"]["authority"] = json!({"channel":"source-preview","receipt_sha256":hash(&bytes),"bundle_sha256":"b".repeat(64),"authority_sha":"f".repeat(40),"release_id":1});
    fs::write(root.join("context.json"), serde_json::to_vec(&context)?)?;
    let output = invoke(root)?;
    assert!(
        output.status.success(),
        "valid preview rejected: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_file(root.join("output.json"))?;
    for (pointer, value) in [
        ("/sdk/authority/receipt_sha256", json!("0".repeat(64))),
        ("/sdk/authority/bundle_sha256", json!("0".repeat(64))),
        ("/sdk/authority/release_id", json!(2)),
        ("/sdk/asset_sha256", json!("0".repeat(64))),
        ("/sdk/source_commit", json!("d".repeat(40))),
    ] {
        let mut changed = context.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or("missing fixture field")? = value;
        fs::write(root.join("context.json"), serde_json::to_vec(&changed)?)?;
        assert!(
            !invoke(root)?.status.success(),
            "invalid preview {pointer} accepted"
        );
        assert!(!root.join("output.json").exists());
    }
    fs::write(root.join("context.json"), serde_json::to_vec(&context)?)?;
    lock.remove("runtime-preview");
    lock.insert("planner-preview".into(), toml::Value::Table(record));
    fs::write(root.join("generator.lock"), toml::to_string(&lock)?)?;
    assert!(
        !invoke(root)?.status.success(),
        "planner-only approval accepted for runtime"
    );
    assert!(!root.join("output.json").exists());
    Ok(())
}
