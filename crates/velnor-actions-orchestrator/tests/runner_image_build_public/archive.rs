// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    io::{Cursor, Read},
};

const ORIGINAL: &[u8] =
    include_bytes!("../../../velnor-archive-guard/tests/fixtures/buildkit-attested-probe.oci.tar");
const RUNNER: &str =
    include_str!("../../../velnor-archive-guard/tests/fixtures/runner-family/buildkit-worker.json");
const DIND: &str =
    include_str!("../../../velnor-archive-guard/tests/fixtures/runner-family/buildkit-dind.json");
const PROBE: &str =
    include_str!("../../../velnor-archive-guard/tests/fixtures/runner-family/buildkit-probe.json");

pub(super) fn make(role: &str, source: &str, revision: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut files = read_archive()?;
    let mut outer: Value = serde_json::from_slice(files.get("index.json").ok_or("index")?)?;
    let nested_key = key(&outer["manifests"][0]["digest"])?;
    let mut nested: Value = serde_json::from_slice(files.get(&nested_key).ok_or("nested")?)?;
    let platform_key = key(&nested["manifests"][0]["digest"])?;
    let mut platform_manifest: Value =
        serde_json::from_slice(files.get(&platform_key).ok_or("platform manifest")?)?;
    let config_key = key(&platform_manifest["config"]["digest"])?;
    let mut config: Value = serde_json::from_slice(files.get(&config_key).ok_or("config")?)?;
    config["config"] = match role {
        "runner" => {
            json!({"User":"runner","Entrypoint":["/usr/local/bin/velnor-runner-entrypoint"],"Labels":{"org.opencontainers.image.revision":source,"io.velnor.image.kind":"runner"}})
        }
        "dind" => {
            json!({"User":"","Entrypoint":["/usr/local/bin/velnor-dind-entrypoint"],"Labels":{"org.opencontainers.image.revision":source,"io.velnor.image.kind":"dind"}})
        }
        "resource_probe" => {
            json!({"User":"65532:65532","WorkingDir":"/","Entrypoint":["/velnor/resource-probe"],"Env":["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],"Labels":{"org.opencontainers.image.revision":source}})
        }
        _ => return Err("role".into()),
    };
    replace_blob(
        &mut files,
        &config_key,
        &mut platform_manifest["config"],
        serde_json::to_vec(&config)?,
    );
    replace_blob(
        &mut files,
        &platform_key,
        &mut nested["manifests"][0],
        serde_json::to_vec(&platform_manifest)?,
    );
    let platform = nested["manifests"][0]["digest"]
        .as_str()
        .ok_or("platform")?
        .to_owned();
    let manifest_key = key(&nested["manifests"][1]["digest"])?;
    let mut manifest: Value = serde_json::from_slice(files.get(&manifest_key).ok_or("manifest")?)?;
    manifest["subject"]["digest"] = json!(platform);
    manifest["subject"]["size"] = nested["manifests"][0]["size"].clone();
    nested["manifests"][1]["annotations"]["vnd.docker.reference.digest"] = json!(platform);
    let statement_key = key(&manifest["layers"][0]["digest"])?;
    let statement = statement(role, source, revision, &platform)?;
    replace_blob(
        &mut files,
        &statement_key,
        &mut manifest["layers"][0],
        serde_json::to_vec(&statement)?,
    );
    replace_blob(
        &mut files,
        &manifest_key,
        &mut nested["manifests"][1],
        serde_json::to_vec(&manifest)?,
    );
    replace_blob(
        &mut files,
        &nested_key,
        &mut outer["manifests"][0],
        serde_json::to_vec(&nested)?,
    );
    files.insert("index.json".into(), serde_json::to_vec(&outer)?);
    let mut tar = tar::Builder::new(Vec::new());
    for (name, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, bytes.as_slice())?;
    }
    Ok(tar.into_inner()?)
}
fn read_archive() -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let mut files = BTreeMap::new();
    for entry in tar::Archive::new(Cursor::new(ORIGINAL)).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        files.insert(name, bytes);
    }
    Ok(files)
}

fn statement(
    role: &str,
    source: &str,
    revision: &str,
    platform: &str,
) -> Result<Value, Box<dyn Error>> {
    let template = match role {
        "runner" => RUNNER,
        "dind" => DIND,
        "resource_probe" => PROBE,
        _ => return Err("role".into()),
    };
    let mut statement: Value = serde_json::from_str(template)?;
    statement["subject"][0]["digest"]["sha256"] =
        json!(platform.strip_prefix("sha256:").ok_or("digest")?);
    statement["predicate"]["buildDefinition"]["internalParameters"]["builderPlatform"] =
        json!("linux/amd64");
    let argument = if role == "resource_probe" {
        "build-arg:SOURCE_SHA"
    } else {
        "build-arg:VELNOR_SOURCE_SHA"
    };
    statement["predicate"]["buildDefinition"]["externalParameters"]["request"]["args"][argument] =
        json!(source);
    let vcs = statement["predicate"]["runDetails"]["metadata"]["buildkit_metadata"]["vcs"]
        .as_object_mut()
        .ok_or("vcs")?;
    vcs.insert("revision".into(), json!(revision));
    let names: &[&str] = match role {
        "runner" => &[
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ],
        "dind" | "resource_probe" => &["resource_probe"],
        _ => return Err("role".into()),
    };
    for name in names {
        vcs.insert(
            format!("localdir:{name}"),
            json!(format!("target/velnor-runner-family/{name}")),
        );
    }
    let vcs = vcs.clone();
    let request = statement["predicate"]["buildDefinition"]["externalParameters"]["request"]
        .as_object_mut()
        .ok_or("request")?;
    let mut args = request["args"].as_object().ok_or("args")?.clone();
    for (k, v) in vcs {
        args.insert(format!("vcs:{k}"), v);
    }
    request["root"]["request"]["args"] = Value::Object(args);
    Ok(statement)
}

fn key(value: &Value) -> Result<String, Box<dyn Error>> {
    Ok(format!(
        "blobs/{}",
        value.as_str().ok_or("digest")?.replace(':', "/")
    ))
}
fn replace_blob(
    files: &mut BTreeMap<String, Vec<u8>>,
    old: &str,
    descriptor: &mut Value,
    bytes: Vec<u8>,
) {
    files.remove(old);
    let digest = super::hex(&Sha256::digest(&bytes));
    descriptor["digest"] = json!(format!("sha256:{digest}"));
    descriptor["size"] = json!(bytes.len());
    files.insert(format!("blobs/sha256/{digest}"), bytes);
}
