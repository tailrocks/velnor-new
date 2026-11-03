// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::{Value, json};
use velnor_actions_contract::cachekey::{
    FormatInputs, cache_format_id, mbx_qualification_cache_generation,
};
use velnor_actions_mise::PinnedTool;
use velnor_actions_orchestrator::{plan_internal, prepare};

use super::one_job_per_crate::mbx_repo;
use super::workflow_configuration_fixture::{self, git_fixture};

const LINEAGE_FORMAT_LABEL: &str = "velnor-cache-v1";

fn lineage_format_id(generation: String) -> Result<String, Box<dyn Error>> {
    Ok(cache_format_id(&FormatInputs {
        adapter: "mbx".to_owned(),
        format: LINEAGE_FORMAT_LABEL.to_owned(),
        generation,
    })?)
}

#[test]
fn mbx_lineage_format_id_comes_from_the_qualification_generation() -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    let root = repo.path();
    workflow_configuration_fixture::git(&["add", "-A"], root)?;
    workflow_configuration_fixture::git(&["commit", "-qm", "base"], root)?;
    let head = String::from_utf8(
        git_fixture::command(root)?
            .args(["rev-parse", "HEAD"])
            .output()?
            .stdout,
    )?;
    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r190001-a1",
        "head": head.trim(),
        "event": "push",
        "root": root.to_string_lossy(),
    });
    let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    let version = prepare(root)?
        .catalog
        .version(PinnedTool::MrBoxington)
        .to_owned();
    let expected = lineage_format_id(mbx_qualification_cache_generation(&version))?;
    let flags_enabled = lineage_format_id(format!(
        "velnor-mbx-{version}-share-out-dir-build-scripts-enabled-v1"
    ))?;
    assert_ne!(expected, flags_enabled);
    let mut seen = 0;
    for entry in response["matrix"]["include"]
        .as_array()
        .ok_or("matrix include missing")?
    {
        if entry["adapter_metadata"]["compile_driver"] != "mbx" {
            continue;
        }
        seen += 1;
        let got = entry["cache_ids"]["cache_format_id"]
            .as_str()
            .ok_or("cache_format_id missing")?;
        assert_eq!(
            got, expected,
            "lineage format id must derive from the flag-off qualification slot"
        );
    }
    assert!(seen > 0, "no mbx entries planned");
    Ok(())
}
