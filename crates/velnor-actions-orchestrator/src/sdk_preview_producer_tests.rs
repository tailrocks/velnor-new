// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::error::Error;
use velnor_actions_contract::preview_production::{PayloadArtifactRef, release_plan_sha256};

pub(super) fn task() -> Result<Task, Box<dyn Error>> {
    let plan: velnor_actions_contract::PackageReleasePlan = serde_json::from_value(
        serde_json::json!({
            "schema":"velnor.package-release-plan.v1","channel":"preview","repository":"tailrocks/velnor-new","default_branch":"main","base_version":env!("CARGO_PKG_VERSION"),
            "products":[{"id":"velnor-actions","package":"velnor-actions-cli","bins":["velnor-actions"],"targets":["x86_64-unknown-linux-gnu"],"format":"tar-gz"},{"id":"velnor-archive-guard","package":"velnor-archive-guard","bins":["velnor-archive-guard"],"targets":["x86_64-unknown-linux-gnu"],"format":"tar-gz"}],"companions":[]
        }),
    )?;
    let request: ReleaseProductionRequest = serde_json::from_value(serde_json::json!({
        "schema":"velnor.release-production.v1","policy_sha256":"a".repeat(64),"plan_sha256":release_plan_sha256(&plan)?,"plan":plan,
        "context":{"repository_id":1,"head_repository_id":1,"source_sha":"a".repeat(40),"source_ref":"refs/heads/main","workflow_authority_sha":"b".repeat(40),"run_id":1,"run_attempt":1,"ci_run_id":2,"ci_run_attempt":1,"commit_count":3,"version":format!("{}-preview.3+aaaaaaa",env!("CARGO_PKG_VERSION"))},
        "stage":{"kind":"payload","pair":{"product_id":"velnor-actions","target":"x86_64-unknown-linux-gnu"}}
    }))?;
    Ok(Task {
        schema: "velnor.release-task.v1".into(),
        request,
        inputs: Vec::new(),
    })
}

fn directory(parent: &Path, name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let path = parent.join(name);
    fs::create_dir(&path)?;
    Ok(path)
}

#[test]
fn sdk_payload_archives_supplied_bytes_deterministically_and_assembly_preserves_them()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let binary = temp.path().join("binary");
    fs::write(&binary, b"archive encoding fixture")?;
    let mut task = task()?;
    let first = directory(temp.path(), "first")?;
    let second = directory(temp.path(), "second")?;
    package(&task, &first, Some(&binary))?;
    package(&task, &second, Some(&binary))?;
    let ReleaseProductionStage::Payload { pair } = &task.request.stage else {
        return Err("expected payload stage".into());
    };
    let pair = pair.clone();
    let name = format!("{}-{}.tar.gz", pair.product_id, pair.target);
    let archive = fs::read(first.join(&name))?;
    assert_eq!(archive, fs::read(second.join(&name))?);
    let members = crate::package_release_artifacts::extract_tar_gz_members(
        &archive,
        &std::collections::BTreeSet::from(["velnor-actions".into()]),
        std::time::Instant::now(),
    )?;
    assert_eq!(members["velnor-actions"], fs::read(&binary)?);
    let guard_pair = ReleasePayloadPair {
        product_id: "velnor-archive-guard".into(),
        target: pair.target.clone(),
    };
    task.request.stage = ReleaseProductionStage::Payload {
        pair: guard_pair.clone(),
    };
    let guard_output = directory(temp.path(), "guard")?;
    package(&task, &guard_output, Some(&binary))?;
    let mut leaves = Vec::new();
    for (index, (pair, output)) in [(pair, first), (guard_pair, guard_output)]
        .into_iter()
        .enumerate()
    {
        let name = format!("{}-{}.tar.gz", pair.product_id, pair.target);
        for name in [name.clone(), format!("{name}.sbom.json")] {
            let path = output.join(&name);
            let bytes = fs::read(&path)?;
            task.inputs.push(Input {
                pair: pair.clone(),
                name,
                sha256: digest(&bytes),
                size: bytes.len() as u64,
                path: path.to_str().ok_or("fixture path is not UTF-8")?.into(),
            });
        }
        leaves.push(PayloadArtifactRef {
            pair,
            artifact_id: index as u64 + 1,
            sha256: "c".repeat(64),
        });
    }
    task.request.stage = ReleaseProductionStage::Assemble { inputs: leaves };
    let aggregate = directory(temp.path(), "aggregate")?;
    package(&task, &aggregate, None)?;
    for input in &task.inputs {
        assert_eq!(
            fs::read(aggregate.join(&input.name))?,
            fs::read(&input.path)?
        );
    }
    let manifest: PackageOutputManifest =
        serde_json::from_slice(&fs::read(aggregate.join("package-output.json"))?)?;
    assert_eq!(manifest.schema, "velnor.package-output.v1");
    assert_eq!(manifest.source_commit, task.request.context.source_sha);
    task.inputs[0].sha256 = "d".repeat(64);
    assert!(package(&task, &directory(temp.path(), "corrupt")?, None).is_err());
    task.inputs.pop();
    assert!(package(&task, &directory(temp.path(), "partial")?, None).is_err());
    Ok(())
}

#[test]
fn sdk_payload_rejects_symlink_binary_and_undeclared_product() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let binary = temp.path().join("binary");
    fs::write(&binary, b"fixture")?;
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&binary, &link)?;
    let mut task = task()?;
    assert!(package(&task, &directory(temp.path(), "symlink")?, Some(&link)).is_err());
    task.request.plan.products[0].id = "other".into();
    task.request.plan_sha256 = release_plan_sha256(&task.request.plan)?;
    assert!(package(&task, &directory(temp.path(), "wrong")?, Some(&binary)).is_err());
    Ok(())
}

#[test]
fn sdk_preview_bootstrap_verifies_external_release_boundaries_before_staging()
-> Result<(), Box<dyn Error>> {
    let script = include_str!("../../../scripts/test_sdk_preview_bootstrap.py");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("python3")
        .args(["-c", script])
        .env("VELNOR_SDK_BOOTSTRAP_TEST_ROOT", root)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
