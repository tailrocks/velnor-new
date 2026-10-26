// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, io};

use velnor_actions_contract::{PullRequestCachePolicy, ValidatorKind, WorkflowPolicy};
use velnor_actions_orchestrator::{GenerationPreparation, OrchestratorError, finalized_jobs};
use velnor_actions_workflow_renderer::{RenderError, render::render_workflow_ir};

pub(crate) fn assert_native_context_rejections(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    assert_positive(preparation, policy)?;

    assert_context_mutation_rejected(preparation, policy, "generator version", |context| {
        context.generator_version = if context.generator_version == "9.9.9" {
            "9.9.8".to_owned()
        } else {
            "9.9.9".to_owned()
        };
    })?;
    assert_context_mutation_rejected(
        preparation,
        policy,
        "pull request cache policy",
        |context| {
            context.pull_request_cache_policy = match context.pull_request_cache_policy {
                PullRequestCachePolicy::ReadOnly => PullRequestCachePolicy::SameRepositoryScoped,
                PullRequestCachePolicy::SameRepositoryScoped => PullRequestCachePolicy::ReadOnly,
            };
        },
    )?;
    assert_context_mutation_rejected(preparation, policy, "Mise quiet mode", |context| {
        context.mise_quiet = !context.mise_quiet;
    })?;
    if !preparation.workflow.context.tool_pins.is_empty() {
        assert_context_mutation_rejected(preparation, policy, "declared tool pin", |context| {
            let pin = &mut context.tool_pins[0];
            let mut fields = pin.split(' ').map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(fields.len(), 5, "fixture tool pin has an unexpected shape");
            fields[1] = if fields[1] == "9.9.9" {
                "9.9.8".to_owned()
            } else {
                "9.9.9".to_owned()
            };
            *pin = fields.join(" ");
        })?;
    }

    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        let command_index = preparation
            .workflow
            .context
            .validator_commands
            .iter()
            .position(|command| {
                matches!(
                    command.validator,
                    ValidatorKind::CargoDeny | ValidatorKind::CargoMachete
                )
            })
            .ok_or_else(|| {
                io::Error::other(
                    "producer fixture must contain a CargoDeny or CargoMachete command",
                )
            })?;
        assert_context_mutation_rejected(
            preparation,
            policy,
            "native validator argv",
            |context| {
                let command = &mut context.validator_commands[command_index];
                command.argv.push("--candidate-binding-probe".to_owned());
            },
        )?;
    }
    Ok(())
}

fn assert_positive(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    render_workflow_ir(
        &preparation.workflow.ir,
        policy,
        preparation.workflow.support.as_ref(),
        &preparation.workflow.context,
    )?;
    finalized_jobs(preparation)?;
    Ok(())
}

fn assert_context_mutation_rejected(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
    field: &str,
    mutate: impl FnOnce(&mut velnor_actions_workflow_renderer::render::RenderContext),
) -> Result<(), Box<dyn Error>> {
    let mut changed = preparation.clone();
    mutate(&mut changed.workflow.context);
    changed.workflow.ir.validate().map_err(|error| {
        io::Error::other(format!(
            "generic workflow validation rejected {field}: {error}"
        ))
    })?;

    let raw_error = render_workflow_ir(
        &changed.workflow.ir,
        policy,
        changed.workflow.support.as_ref(),
        &changed.workflow.context,
    )
    .err()
    .ok_or_else(|| io::Error::other(format!("raw render accepted changed {field}")))?;
    assert!(
        matches!(&raw_error, RenderError::InvalidWorkflow(problem) if problem == "native_authority_graph"),
        "unexpected raw-render error for {field}: {raw_error:?}"
    );

    let strict_error = finalized_jobs(&changed)
        .err()
        .ok_or_else(|| io::Error::other(format!("finalizer accepted changed {field}")))?;
    assert!(
        matches!(&strict_error, OrchestratorError::Render { problem } if problem == "invalid workflow: native_authority_graph"),
        "unexpected finalizer error for {field}: {strict_error}"
    );
    Ok(())
}
