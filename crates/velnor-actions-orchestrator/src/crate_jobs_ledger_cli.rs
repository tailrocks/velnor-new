// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::ledger_cli::{
    LEDGER_CLI_ARTIFACT_NAME, LEDGER_CLI_ARTIFACT_OUTPUT, LEDGER_CLI_DISPLAY_NAME,
    LEDGER_CLI_SHA256_OUTPUT, LEDGER_CLI_SHA256_SOURCE, LEDGER_CLI_SOURCE_SHA_OUTPUT,
    LEDGER_CLI_SOURCE_SHA_SOURCE,
};
use velnor_actions_contract::{CacheMode, Job, JobTimeout, Permissions};

use super::CrateJobInputs;
use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;

pub(super) fn ledger_cli_job(
    input: &CrateJobInputs<'_>,
    private: Option<&PrivateWiring>,
) -> Result<Job, OrchestratorError> {
    let &CrateJobInputs {
        label, fetch_roots, ..
    } = input;
    let catalog = &input.catalog.clone().with_rust_mr_boxington(false);
    let roots = crate::source_prep::crate_fetch_roots("", fetch_roots, false);
    if roots.is_empty() {
        return Err(OrchestratorError::Contract {
            problem: "ledger_cli_requires_root_cargo_lock".to_owned(),
        });
    }
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.extend(input.acquire.cloned());
    steps.push(crate::workflow::verify_restored_rust_step(catalog));
    steps.push(crate::matrix_step::prepare_crate_tools_step_with_bun(
        catalog, true, false, false, false, false,
    )?);
    steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    steps.push(crate::workflow::verify_rust_manifest_step(catalog));
    steps.extend(crate::source_cache::keyed_restore_steps(catalog, &roots)?);
    if let Some(private) = private {
        steps.extend(private.steps_for_roots(&roots)?);
    }
    steps.extend(crate::source_prep::fetch_steps_for_crate(
        catalog, &roots, false,
    )?);
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    steps.push(
        velnor_actions_workflow_renderer::ledger_cli_build_step(catalog, &env)
            .map_err(OrchestratorError::from)?,
    );
    steps.push(
        velnor_actions_workflow_renderer::ledger_cli_publish_step()
            .map_err(OrchestratorError::from)?,
    );
    steps.push(
        velnor_actions_workflow_renderer::ledger_cli_upload_step()
            .map_err(OrchestratorError::from)?,
    );
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::from([
            (
                LEDGER_CLI_ARTIFACT_OUTPUT.to_owned(),
                LEDGER_CLI_ARTIFACT_NAME.to_owned(),
            ),
            (
                LEDGER_CLI_SHA256_OUTPUT.to_owned(),
                LEDGER_CLI_SHA256_SOURCE.to_owned(),
            ),
            (
                LEDGER_CLI_SOURCE_SHA_OUTPUT.to_owned(),
                LEDGER_CLI_SOURCE_SHA_SOURCE.to_owned(),
            ),
        ]),
        display_name: LEDGER_CLI_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PRODUCT_COMPILER,
        needs: Vec::new(),
        condition: None,
        permissions: Some(Permissions::default()),
        environment: None,
        steps,
        cache_mode: Some(CacheMode::explicit_read()),
    })
}
