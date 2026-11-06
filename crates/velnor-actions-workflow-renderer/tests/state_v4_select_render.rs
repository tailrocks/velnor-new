// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::Command;
use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::workflow::PermissionLevel;
use velnor_actions_contract::workflow::StepKind;
use velnor_actions_contract::workflow::cache_family::{
    ROOT_SLUG_MAX_BYTES, RootSlug, S256, StateScope, StateTrust, StateUnit,
};
use velnor_actions_contract::workflow::state_seal::{state_attested_output, state_produce_output};
use velnor_actions_contract::workflow::step_identity::StepRole;
use velnor_actions_workflow_renderer::plan_sources::cache_keys::StateRootPlan;
use velnor_actions_workflow_renderer::plan_sources::state_select::{
    SealRoot, StateSealPlan, state_select_step,
};
use velnor_actions_workflow_renderer::render::render_workflow_ir;
use velnor_actions_workflow_renderer::{
    join_argv_for_run,
    native_job_builders::{PlanJobToolNeeds, plan_job, plan_state_roots},
    validate_command_argv,
};

use crate::gc1_t41::{context, emitted_selector_run, fail, native_check_step, workflow};

#[test]
fn rendered_selector_yaml_accepts_zero_one_and_65_roots() -> Result<(), Box<dyn Error>> {
    rendered_selector_case(&[])?;
    let one_root = vec![SealRoot {
        unit: StateUnit::Union,
        slug: RootSlug::from_root(".")?,
    }];
    rendered_selector_case(&one_root)?;
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
    rendered_selector_case(&roots)?;
    Ok(())
}

#[test]
fn rendered_selector_uses_the_largest_public_root_count_within_existing_bounds()
-> Result<(), Box<dyn Error>> {
    let mut accepted = 0;
    let mut rejected = 1;
    while rendered_short_roots(rejected).is_ok() {
        accepted = rejected;
        rejected = rejected
            .checked_mul(2)
            .ok_or_else(|| fail("selector root count search overflowed"))?;
    }
    while accepted + 1 < rejected {
        let candidate = accepted + (rejected - accepted) / 2;
        if rendered_short_roots(candidate).is_ok() {
            accepted = candidate;
        } else {
            rejected = candidate;
        }
    }
    let final_case = rendered_short_roots(accepted)?;
    let first_rejected = rendered_short_roots(accepted + 1)
        .expect_err("the next short-slug selector must exceed an existing bound")
        .to_string();
    if accepted == 0
        || final_case.workflow_bytes > velnor_actions_workflow_renderer::MAX_WORKFLOW_BYTES
        || final_case.run.encode_utf16().count() > 21_000
        || !(first_rejected.contains("workflow_too_large:")
            || first_rejected.contains("state_select:run_too_large:21000_utf16_code_units"))
    {
        return Err(fail(
            "selector root boundary did not match an existing limit",
        ));
    }
    println!(
        "adaptive selector boundary max_roots={accepted} workflow_bytes={} run_utf16_units={} next_roots={} rejection={first_rejected}",
        final_case.workflow_bytes,
        final_case.run.encode_utf16().count(),
        accepted + 1
    );

    let fixed_valid = rendered_short_roots(1_000)?;
    let fixed_valid_run_units = fixed_valid.run.encode_utf16().count();
    if fixed_valid.workflow_bytes > velnor_actions_workflow_renderer::MAX_WORKFLOW_BYTES
        || fixed_valid_run_units > 21_000
    {
        return Err(fail(&format!(
            "fixed valid selector fixture exceeds existing bounds: roots=1000 workflow_bytes={} run_utf16_units={fixed_valid_run_units}",
            fixed_valid.workflow_bytes
        )));
    }
    let fixed_oversized = rendered_short_roots(2_050)
        .expect_err("the fixed oversized fixture must cross the document boundary");
    if !fixed_oversized.contains("workflow_too_large:") {
        return Err(fail(&format!(
            "fixed oversized selector fixture did not reach the document guard: roots=2050 rejection={fixed_oversized}"
        )));
    }
    println!(
        "fixed valid selector fixture roots=1000 workflow_bytes={} run_utf16_units={fixed_valid_run_units}",
        fixed_valid.workflow_bytes
    );
    println!("fixed oversized selector fixture roots=2050 rejection={fixed_oversized}");
    Ok(())
}

#[derive(Debug)]
struct RenderedSelector {
    run: String,
    workflow_bytes: usize,
}

fn rendered_short_roots(count: usize) -> Result<RenderedSelector, Box<dyn Error>> {
    let roots = (0..count)
        .map(|ordinal| {
            Ok(SealRoot {
                unit: if ordinal % 2 == 0 {
                    StateUnit::Union
                } else {
                    StateUnit::Suite
                },
                slug: RootSlug::from_root(&short_slug(ordinal))?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    rendered_selector_case(&roots)
}

fn short_slug(mut ordinal: usize) -> String {
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut width = 1_u32;
    loop {
        let capacity = alphabet.len().pow(width);
        if ordinal < capacity {
            let mut slug = vec![b'a'; width as usize];
            for byte in slug.iter_mut().rev() {
                *byte = alphabet[ordinal % alphabet.len()];
                ordinal /= alphabet.len();
            }
            return String::from_utf8(slug).expect("slug alphabet is ASCII");
        }
        ordinal -= capacity;
        width += 1;
    }
}

fn rendered_selector_case(roots: &[SealRoot]) -> Result<RenderedSelector, Box<dyn Error>> {
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
    validate_command_argv(run)?;
    if run.get(0).map(String::as_str) != Some("exec")
        || run.get(1).map(String::as_str) != Some("sh")
        || run.get(2).map(String::as_str) != Some("-c")
    {
        return Err(fail("selector is not the expected exec sh -c command"));
    }
    let inner_script = run
        .get(3)
        .map(String::as_str)
        .ok_or_else(|| fail("selector is missing its inner sh -c script"))?;
    let syntax = Command::new("sh")
        .arg("-n")
        .arg("-c")
        .arg(inner_script)
        .status()?;
    if !syntax.success() {
        return Err(fail("exact selector inner script failed sh -n"));
    }
    let expected_run = join_argv_for_run(run)?;
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
    Ok(RenderedSelector {
        run: expected_run,
        workflow_bytes: yaml.len(),
    })
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
    if roots.is_empty() {
        let mut permissions =
            velnor_actions_workflow_renderer::native_job_builders::read_actions_permissions();
        permissions.attest = PermissionLevel::Read;
        plan.permissions = Some(permissions);
        let selector = state_select_step(&[], JobTimeout::PLAN)?;
        let at = plan.steps.len().saturating_sub(1);
        plan.steps.insert(at, selector);
    } else {
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
    }
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
