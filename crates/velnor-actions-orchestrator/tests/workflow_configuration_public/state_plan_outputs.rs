// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::workflow::cache_family::is_plan_cache_output;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_WORKFLOW_PATH;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::one_job_per_crate::mbx_repo;
use super::workflow_configuration_fixture::add_independent_mbx_package;

#[test]
fn plan_publishes_four_distinct_outputs_per_adversarial_unit() -> Result<(), Box<dyn Error>> {
    const ADVERSARIAL: [&str; 4] = ["a", "a-b", "cli", "suite-a"];
    let repo = mbx_repo(false)?;
    for dir in ADVERSARIAL {
        add_independent_mbx_package(repo.path(), dir, dir)?;
    }
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    let mut published: BTreeSet<String> = BTreeSet::new();
    for path in [WORKFLOW_PATH, STATE_SEAL_WORKFLOW_PATH] {
        let workflow: serde_json::Value =
            serde_saphyr::from_str(tree.get(path).ok_or("a workflow was not rendered")?)?;
        let plan = &workflow["jobs"]["plan"];
        published.extend(
            plan["outputs"]
                .as_object()
                .ok_or("plan job has no outputs")?
                .keys()
                .filter(|name| name.starts_with("state_"))
                .cloned(),
        );
        let text = serde_json::to_string(&plan["steps"])?;
        let mut rest = text.as_str();
        while let Some(at) = rest.find("steps.cache-keys.outputs.state_") {
            rest = &rest[at + "steps.cache-keys.outputs.".len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            published.insert(rest[..end].to_owned());
        }
    }
    let mut expected: BTreeSet<String> = BTreeSet::new();
    for dir in ADVERSARIAL {
        let unit = format!("slice_rust_{}", dir.replace('-', "_"));
        let names = ["attested", "compat", "snapshot"]
            .map(|kind| format!("state_{unit}_{kind}"))
            .into_iter()
            .chain([format!("state_matrix_producer_{}", dir.replace('-', "_"))]);
        for name in names {
            if !published.contains(&name) {
                return Err(format!("plan never declares {name}: {published:?}").into());
            }
            if !is_plan_cache_output(&name) {
                return Err(format!("{name} is not a recognised plan cache output").into());
            }
            expected.insert(name);
        }
    }
    if expected.len() != 16 {
        return Err(format!("unit outputs collide: {expected:?}").into());
    }
    if let Some(unknown) = published.iter().find(|name| !is_plan_cache_output(name)) {
        return Err(format!("plan declares an unrecognised state output {unknown}").into());
    }
    if let Some(stray) = published
        .iter()
        .find(|name| name.starts_with("state_cli_") || name.starts_with("state_suite_"))
    {
        return Err(format!("a unit without a producer is published: {stray}").into());
    }
    Ok(())
}
