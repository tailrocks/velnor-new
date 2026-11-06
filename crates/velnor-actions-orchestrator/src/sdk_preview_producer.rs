// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "sdk_preview_build.rs"]
mod build;
use build::build_qualified;

use crate::{
    OrchestratorError,
    internal::internal,
    package_release_artifacts::{PackageOutputManifest, PackageOutputPayload, PackageOutputSbom},
};
use serde::Deserialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use velnor_actions_contract::{
    canonical_json_bytes,
    preview_production::{ReleasePayloadPair, ReleaseProductionRequest, ReleaseProductionStage},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Task {
    schema: String,
    request: ReleaseProductionRequest,
    inputs: Vec<Input>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    pair: ReleasePayloadPair,
    name: String,
    sha256: String,
    size: u64,
    path: String,
}

pub fn produce_sdk_preview(task_path: &Path, output: &Path) -> Result<(), OrchestratorError> {
    let bytes = read(task_path, 1024 * 1024)?;
    let value = velnor_actions_contract::parse_strict_json(
        std::str::from_utf8(&bytes).map_err(|_| internal("sdk_task_utf8"))?,
    )?;
    let task: Task = serde_json::from_value(value).map_err(|_| internal("sdk_task_schema"))?;
    validate_task(&task)?;
    let binary = match &task.request.stage {
        ReleaseProductionStage::Payload { pair } => Some(build_qualified(
            &task.request,
            &pair.product_id,
            task_path
                .parent()
                .filter(|path| path.is_absolute())
                .ok_or_else(|| internal("sdk_task_parent"))?,
        )?),
        ReleaseProductionStage::Assemble { .. } => None,
    };
    package(
        &task,
        output,
        binary.as_ref().map(|binary| binary.path.as_path()),
    )
}

fn package(task: &Task, output: &Path, binary: Option<&Path>) -> Result<(), OrchestratorError> {
    validate_task(task)?;
    let metadata = fs::symlink_metadata(output).map_err(|_| internal("sdk_output_missing"))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || fs::read_dir(output)
            .map_err(|_| internal("sdk_output_read"))?
            .next()
            .is_some()
    {
        return Err(internal("sdk_output_not_empty_directory"));
    }
    let mut manifest = PackageOutputManifest {
        schema: "velnor.package-output.v1".into(),
        source_commit: task.request.context.source_sha.clone(),
        source_ref: task.request.context.source_ref.clone(),
        version: task.request.context.version.clone(),
        payloads: Vec::new(),
        companions: Vec::new(),
        sboms: Vec::new(),
    };
    match &task.request.stage {
        ReleaseProductionStage::Payload { pair } => {
            if !task.inputs.is_empty() {
                return Err(internal("sdk_payload_inputs"));
            }
            manifest.schema = "velnor.package-payload-output.v1".into();
            let binary = binary.ok_or_else(|| internal("sdk_binary_missing"))?;
            let data = read(binary, 256 * 1024 * 1024)?;
            let name = format!("{}-{}.tar.gz", pair.product_id, pair.target);
            let archive = archive_bytes(&pair.product_id, &data)?;
            write(&output.join(&name), &archive)?;
            let digest = digest(&archive);
            let sbom = serde_json::json!({"bomFormat":"CycloneDX","specVersion":"1.6","version":1,"metadata":{"component":{"type":"file","name":name,"hashes":[{"alg":"SHA-256","content":digest}]}}});
            write(
                &output.join(format!("{name}.sbom.json")),
                &canonical_json_bytes(&sbom)?,
            )?;
            add(&mut manifest, pair, &name, output)?;
        }
        ReleaseProductionStage::Assemble { inputs } => {
            if binary.is_some() || task.inputs.len() != inputs.len() * 2 {
                return Err(internal("sdk_aggregate_inputs"));
            }
            let mut names = std::collections::BTreeSet::new();
            for input in &task.inputs {
                let base = format!("{}-{}.tar.gz", input.pair.product_id, input.pair.target);
                if !inputs.iter().any(|i| i.pair == input.pair)
                    || (input.name != base && input.name != format!("{base}.sbom.json"))
                    || !names.insert(input.name.clone())
                {
                    return Err(internal("sdk_aggregate_inventory"));
                }
                let data = read(&PathBuf::from(&input.path), 1024 * 1024 * 1024)?;
                if data.len() as u64 != input.size || digest(&data) != input.sha256 {
                    return Err(internal("sdk_aggregate_digest"));
                }
                write(&output.join(&input.name), &data)?;
            }
            for input in inputs {
                let name = format!("{}-{}.tar.gz", input.pair.product_id, input.pair.target);
                add(&mut manifest, &input.pair, &name, output)?;
            }
        }
    }
    write(
        &output.join("package-output.json"),
        &canonical_json_bytes(&manifest)?,
    )
}

fn add(
    manifest: &mut PackageOutputManifest,
    pair: &ReleasePayloadPair,
    name: &str,
    output: &Path,
) -> Result<(), OrchestratorError> {
    let archive = read(&output.join(name), 1024 * 1024 * 1024)?;
    let sbom_name = format!("{name}.sbom.json");
    let sbom = read(&output.join(&sbom_name), 32 * 1024 * 1024)?;
    manifest.payloads.push(PackageOutputPayload {
        product_id: pair.product_id.clone(),
        target: pair.target.clone(),
        format: velnor_actions_contract::package_release::PackageReleasePayloadFormat::TarGz,
        name: name.into(),
        sha256: digest(&archive),
    });
    manifest.sboms.push(PackageOutputSbom {
        payload_name: name.into(),
        name: sbom_name,
        sha256: digest(&sbom),
    });
    Ok(())
}

fn read(path: &Path, limit: u64) -> Result<Vec<u8>, OrchestratorError> {
    use std::io::Read;
    let mut file = crate::package_release_artifacts::open_regular_nofollow(path)?;
    let metadata = file.metadata().map_err(|_| internal("sdk_input_stat"))?;
    if metadata.len() == 0 || metadata.len() > limit {
        return Err(internal("sdk_input_size"));
    }
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| internal("sdk_input_read"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(internal("sdk_input_changed"));
    }
    Ok(bytes)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| internal("sdk_output_create"))?;
    file.write_all(bytes)
        .map_err(|_| internal("sdk_output_write"))
}

fn digest(bytes: &[u8]) -> String {
    crate::generator::sha256_hex(bytes)
}

fn archive_bytes(name: &str, data: &[u8]) -> Result<Vec<u8>, OrchestratorError> {
    let gzip = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::default());
    let mut archive = tar::Builder::new(gzip);
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    archive
        .append_data(&mut header, name, data)
        .map_err(|_| internal("sdk_archive"))?;
    archive
        .into_inner()
        .map_err(|_| internal("sdk_archive_finish"))?
        .finish()
        .map_err(|_| internal("sdk_gzip_finish"))
}

fn validate_task(task: &Task) -> Result<(), OrchestratorError> {
    task.request.validate()?;
    if task.schema != "velnor.release-task.v1"
        || !task.request.plan.companions.is_empty()
        || task.request.plan.channel
            != velnor_actions_contract::package_release::PackageReleaseChannel::Preview
        || task.request.plan.repository != "tailrocks/velnor-new"
        || task.request.plan.base_version != env!("CARGO_PKG_VERSION")
        || task.request.plan.products.len() != 2
    {
        return Err(internal("sdk_task_policy"));
    }
    for (product, (id, package)) in task.request.plan.products.iter().zip([
        ("velnor-actions", "velnor-actions-cli"),
        ("velnor-archive-guard", "velnor-archive-guard"),
    ]) {
        if product.id != id
            || product.package != package
            || product.bins != [id]
            || product.targets != ["x86_64-unknown-linux-gnu"]
        {
            return Err(internal("sdk_task_product"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "sdk_preview_workflow_tests.rs"]
mod workflow_tests;

#[cfg(test)]
#[path = "sdk_preview_producer_tests.rs"]
mod tests;
