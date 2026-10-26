// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{CrateObligation, Step, StepId, StepRole};

use crate::OrchestratorError;
use crate::internal::internal;

pub(crate) const TOFU_PROVIDER_CACHE_BASE_EXPR: &str =
    velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDER_CACHE_BASE_EXPR;

pub(crate) fn tofu_cache_roots(
    discovery: &crate::discover::Discovery,
) -> Result<Vec<String>, OrchestratorError> {
    let mut roots = std::collections::BTreeSet::new();
    for task in &discovery.proposals {
        if velnor_actions_contract::Stack::from_id(&task.stack_id)
            != Some(velnor_actions_contract::Stack::Tofu)
            || !crate::crate_jobs::is_runnable(task)
        {
            continue;
        }
        let key = task
            .task_id
            .split('/')
            .nth(2)
            .ok_or_else(|| internal("tofu_unparsable_task_id"))?;
        roots.insert(velnor_actions_tofu::root_for_key(key)?);
    }
    Ok(roots.into_iter().collect())
}

pub(crate) fn tofu_provider_cache_path(root: &str) -> Result<String, OrchestratorError> {
    let locator = velnor_actions_tofu::tofu_root_locator(root)?;
    Ok(
        velnor_actions_contract::workflow::cache_family::CacheFamily::TofuProviders
            .spec()
            .path_input(Some(&locator)),
    )
}

pub(crate) fn tofu_providers_restore_step(
    key: &str,
    path: &str,
) -> Result<Step, OrchestratorError> {
    if key.trim().is_empty()
        || !velnor_actions_workflow_renderer::tofu_cache::tofu_providers_path_ok(path)
    {
        return Err(bad_key("bad_provider_restore_identity".to_owned()));
    }
    let mut step = velnor_actions_workflow_renderer::steps::action_step(
        velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDERS_RESTORE_NAME,
        velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDER_ADMISSION_USES,
        std::collections::BTreeMap::from([
            ("cache-key".to_owned(), key.to_owned()),
            ("cache-path".to_owned(), path.to_owned()),
        ]),
    )
    .map_err(OrchestratorError::from)?;
    step.id = Some(StepId::TofuProviders);
    step.role = Some(StepRole::TofuProvidersRestore);
    Ok(step)
}

pub(crate) fn tofu_root_for_obligations(
    obligations: &[CrateObligation],
) -> Result<String, OrchestratorError> {
    use std::collections::BTreeSet;
    use velnor_actions_tofu::TofuTaskKind;

    let mut tofu = obligations.iter().filter(|obligation| {
        crate::extension_schemas::task_stack_segment(&obligation.task_id)
            == Some(velnor_actions_tofu::STACK_ID)
    });
    let first = tofu
        .next()
        .ok_or_else(|| internal("tofu_empty_obligations"))?;
    let (root, configuration, first_kind) = tofu_obligation_identity(first)?;
    let mut kinds = BTreeSet::from([first_kind.as_str()]);
    let mut identified = vec![(first, first_kind)];

    for obligation in tofu {
        let (other_root, other_configuration, kind) = tofu_obligation_identity(obligation)?;
        if other_root != root || other_configuration != configuration {
            return Err(internal("tofu_mixed_roots"));
        }
        if !kinds.insert(kind.as_str()) {
            return Err(internal("tofu_duplicate_obligation"));
        }
        identified.push((obligation, kind));
    }
    let has_init = kinds.contains(TofuTaskKind::InitForValidate.as_str());
    for (obligation, kind) in identified {
        validate_tofu_obligation_gate(obligation, kind, &root, &configuration, has_init)?;
    }
    Ok(root)
}

fn tofu_obligation_identity(
    obligation: &CrateObligation,
) -> Result<(String, String, velnor_actions_tofu::TofuTaskKind), OrchestratorError> {
    use velnor_actions_tofu::TofuTaskKind;

    let parts: Vec<_> = obligation.task_id.split('/').collect();
    if parts.len() != 5 || parts[0] != "stack" || parts[1] != velnor_actions_tofu::STACK_ID {
        return Err(internal("tofu_unparsable_task_id"));
    }
    let root = velnor_actions_tofu::root_for_key(parts[2])?;
    let kind = TofuTaskKind::parse(parts[3])?;
    if obligation.kind != kind.as_str()
        || velnor_actions_tofu::task_id_for_root(&root, kind, parts[4])? != obligation.task_id
    {
        return Err(internal("tofu_obligation_identity_mismatch"));
    }
    Ok((root, parts[4].to_owned(), kind))
}

fn validate_tofu_obligation_gate(
    obligation: &CrateObligation,
    kind: velnor_actions_tofu::TofuTaskKind,
    root: &str,
    configuration: &str,
    has_init: bool,
) -> Result<(), OrchestratorError> {
    use velnor_actions_tofu::TofuTaskKind;

    let expected = match kind {
        TofuTaskKind::Validate if has_init => vec![velnor_actions_tofu::task_id_for_root(
            root,
            TofuTaskKind::InitForValidate,
            configuration,
        )?],
        TofuTaskKind::Validate | TofuTaskKind::Fmt | TofuTaskKind::InitForValidate => Vec::new(),
    };
    if obligation.gated_by != expected {
        return Err(internal("tofu_obligation_gate_mismatch"));
    }
    Ok(())
}

pub(crate) fn provider_cache_step_for_tofu_root(
    root: &str,
    tofu_roots: &[String],
) -> Result<[Step; 1], OrchestratorError> {
    use velnor_actions_contract::workflow::cache_family::tofu_providers_key_plan_expr;
    let ordinal = tofu_roots
        .iter()
        .position(|candidate| candidate == root)
        .ok_or_else(|| bad_key(format!("tofu_root_without_key:{root}")))?;
    let path = tofu_provider_cache_path(root)?;
    Ok([tofu_providers_restore_step(
        &tofu_providers_key_plan_expr(ordinal),
        &path,
    )?])
}

fn bad_key(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}
