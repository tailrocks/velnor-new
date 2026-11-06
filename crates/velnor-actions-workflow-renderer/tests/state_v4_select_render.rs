// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::workflow::StepKind;
use velnor_actions_contract::workflow::cache_family::{
    ROOT_SLUG_MAX_BYTES, RootSlug, S256, StateScope, StateTrust, StateUnit,
};
use velnor_actions_contract::workflow::state_seal::{state_attested_output, state_produce_output};
use velnor_actions_contract::workflow::step_identity::StepRole;
use velnor_actions_workflow_renderer::native_job_builders::{
    PlanJobToolNeeds, plan_job, plan_state_roots,
};
use velnor_actions_workflow_renderer::plan_sources::cache_keys::StateRootPlan;
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, StateSealPlan};
use velnor_actions_workflow_renderer::render::render_workflow_ir;

use crate::gc1_t41::{context, emitted_selector_run, fail, native_check_step, workflow};

#[test]
fn rendered_selector_yaml_keeps_the_final_run_within_21000_utf16_units()
-> Result<(), Box<dyn Error>> {
    let one_root = vec![SealRoot {
        unit: StateUnit::Union,
        slug: RootSlug::from_root(".")?,
    }];
    let one_run = rendered_selector_case(&one_root)?;
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let roots = (0..65)
        .map(|ordinal| {
            let root = format!(
                "{}{}{}",
                "a".repeat(ROOT_SLUG_MAX_BYTES - 2),
                char::from(alphabet[ordinal / alphabet.len()]),
                char::from(alphabet[ordinal % alphabet.len()]),
            );
            Ok(SealRoot {
                unit: if ordinal % 2 == 0 {
                    StateUnit::Union
                } else {
                    StateUnit::Suite
                },
                slug: RootSlug::from_root(&root)?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let many_run = rendered_selector_case(&roots)?;
    if many_run.encode_utf16().count() > 21_000
        || many_run.encode_utf16().count() > one_run.encode_utf16().count() + 2
        || one_run.encode_utf16().count() > 21_000
    {
        return Err(fail("selector run size grew with its unit count"));
    }
    Ok(())
}

fn rendered_selector_case(roots: &[SealRoot]) -> Result<String, Box<dyn Error>> {
    let plan = planned_job(roots)?;
    let selector = plan
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::StateSelect))
        .cloned()
        .ok_or_else(|| fail("state plan omitted selector step"))?;
    let StepKind::Shell { run, env } = &selector.kind else {
        return Err(fail("selector step is not a shell step"));
    };
    let payload = env
        .get("VELNOR_STATE_SELECT_SUPERVISOR_HEX")
        .ok_or_else(|| fail("selector supervisor environment is absent"))?
        .clone();
    validate_manifest(roots, env)?;
    let expected_run = velnor_actions_workflow_renderer::join_argv_for_run(run)?;
    if expected_run.encode_utf16().count() > 21_000 {
        return Err(fail("selector run exceeds its final UTF-16 bound"));
    }
    let mut ir = workflow(native_check_step(
        "brew-install",
        "internal/native-check/brew-install",
    ));
    ir.jobs.clear();
    ir.jobs.insert("plan".to_owned(), plan);
    let yaml = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &context(&ir.jobs)?)
        .map_err(|error| fail(&format!("render rejected:{error}")))?;
    let rendered_run = emitted_selector_run(&yaml)?;
    if rendered_run != expected_run {
        return Err(fail("emitted selector YAML run changed its argv"));
    }
    if !yaml.contains(&format!("VELNOR_STATE_SELECT_SUPERVISOR_HEX: {payload}")) {
        return Err(fail("selector static environment serialization changed"));
    }
    if yaml.len() > velnor_actions_workflow_renderer::MAX_WORKFLOW_BYTES {
        return Err(fail("selector workflow exceeds its document byte bound"));
    }
    Ok(expected_run)
}

fn planned_job(roots: &[SealRoot]) -> Result<velnor_actions_contract::Job, Box<dyn Error>> {
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let mut plan = plan_job(
        "ubuntu-26.04",
        None,
        &catalog,
        PlanJobToolNeeds::default(),
        &[],
        &[],
        WorkflowPolicy::ConsumerV1,
    )
    .map_err(|error| fail(&format!("plan job rejected:{error:?}")))?;
    let state_roots = roots
        .iter()
        .enumerate()
        .map(|(ordinal, root)| StateRootPlan {
            root: root.slug.as_str().to_owned(),
            pathspecs: vec!["Cargo.toml".to_owned()],
            task_set: S256::of(format!("selector-render-test-task-{ordinal}").as_bytes()),
            producer_recipe: S256::of(format!("selector-render-test-recipe-{ordinal}").as_bytes()),
        })
        .collect::<Vec<_>>();
    plan_state_roots(
        &mut plan,
        &catalog,
        &[],
        &[],
        &state_roots,
        &StateSealPlan {
            roots: roots.to_vec(),
        },
    )
    .map_err(|error| fail(&format!("state plan rejected:{error:?}")))?;
    Ok(plan)
}

fn validate_manifest(
    roots: &[SealRoot],
    env: &std::collections::BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let expected_count = roots.len().to_string();
    if env
        .get("VELNOR_STATE_SELECT_UNIT_COUNT")
        .map(String::as_str)
        != Some(expected_count.as_str())
    {
        return Err(fail("selector unit count differs from its roots"));
    }
    for (ordinal, root) in roots.iter().enumerate() {
        let expected = format!(
            "{} {} {}",
            StateScope {
                trust: StateTrust::Trusted,
                unit: root.unit,
                root: root.slug.clone(),
            }
            .prefix(),
            state_attested_output(root.unit.as_str(), root.slug.as_str()),
            state_produce_output(root.unit.as_str(), root.slug.as_str()),
        );
        if env
            .get(&format!("VELNOR_STATE_SELECT_UNIT_R{ordinal}"))
            .is_none_or(|record| record.len() > 512 || record != &expected)
        {
            return Err(fail(
                "selector unit manifest is missing, misassociated, or too large",
            ));
        }
    }
    Ok(())
}
