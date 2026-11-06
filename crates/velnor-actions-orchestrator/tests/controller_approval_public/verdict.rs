// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, io};

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::{BASE_APPROVALS, BASE_LOCK, fixture};

fn job_block<'a>(workflow: &'a str, job: &str) -> Result<&'a str, Box<dyn Error>> {
    let header = format!("\n  {job}:\n");
    let start = workflow
        .find(&header)
        .ok_or_else(|| io::Error::other(format!("job {job} is not rendered")))?
        + 1;
    let body = &workflow[start..];
    let length = body
        .split_inclusive('\n')
        .enumerate()
        .skip(1)
        .find(|(_, line)| !line.trim().is_empty() && !line.starts_with("    "))
        .map_or(body.len(), |(index, _)| {
            body.split_inclusive('\n').take(index).map(str::len).sum()
        });
    Ok(&body[..length])
}

#[test]
fn required_verdict_controller_is_the_active_approved_release() -> Result<(), Box<dyn Error>> {
    let temp = fixture(Some(BASE_APPROVALS), BASE_LOCK)?;
    super::runtime_public::add_sdk_workspace(temp.path())?;
    let tree = render_staged_tree(&prepare(temp.path())?)?;
    let workflow = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| io::Error::other("ci workflow is not rendered"))?;
    let required = job_block(workflow, "required")?;
    let approvals: serde_json::Value = serde_json::from_str(BASE_APPROVALS)?;
    let active = &approvals["active"];
    let version = active["version"]
        .as_str()
        .ok_or_else(|| io::Error::other("active version is missing"))?;
    let linux = active["binaries"]
        .as_array()
        .and_then(|binaries| {
            binaries
                .iter()
                .find(|binary| binary["target"] == "x86_64-unknown-linux-gnu")
        })
        .ok_or_else(|| io::Error::other("active linux binary is missing"))?;
    let artifact = linux["artifact"]
        .as_str()
        .ok_or_else(|| io::Error::other("active artifact is missing"))?;
    let sha256 = linux["sha256"]
        .as_str()
        .ok_or_else(|| io::Error::other("active sha256 is missing"))?;
    assert!(
        required.contains(&format!("VELNOR_ASSET_URL: {artifact}"))
            && required.contains(&format!("VELNOR_ASSET_SHA256: {sha256}")),
        "Required verdict job does not acquire the active approved release {version}:\n{required}"
    );
    Ok(())
}
