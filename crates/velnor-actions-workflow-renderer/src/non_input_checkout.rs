// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::ledger_cli::LEDGER_CLI_JOB_ID;
use velnor_actions_contract::workflow::state_seal::STATE_PRODUCER_JOB_ID_PREFIX;
use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, Job, NonInputs, Step, StepKind, StepRole, ValidatorKind,
};

use crate::yaml::Yaml;
use crate::{RenderError, steps};

pub const NON_INPUT_GUARD_STEP: &str = "Verify non-input paths are absent";

pub const SPARSE_CHECKOUT_KEY: &str = "sparse-checkout";

pub const SPARSE_CONE_MODE_KEY: &str = "sparse-checkout-cone-mode";

const NAMED_CHECK_JOB_ID_PREFIX: &str = "check-";

pub(crate) const SUITE_JOB_ID_PREFIX: &str = "suite-";

#[must_use]
pub fn consumes_rust_sources(job_id: &str) -> bool {
    job_id.starts_with(CRATE_JOB_ID_PREFIX)
        || job_id.starts_with(NAMED_CHECK_JOB_ID_PREFIX)
        || job_id.starts_with(SUITE_JOB_ID_PREFIX)
        || job_id.starts_with(STATE_PRODUCER_JOB_ID_PREFIX)
        || [
            LEDGER_CLI_JOB_ID,
            ValidatorKind::CargoDeny.job_id(),
            ValidatorKind::CargoMachete.job_id(),
        ]
        .contains(&job_id)
}

pub fn exclude_non_inputs(
    jobs: &mut BTreeMap<String, Job>,
    non_inputs: &NonInputs,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        if consumes_rust_sources(id) {
            exclude_from_job(job, non_inputs)?;
        }
    }
    Ok(())
}

pub fn exclude_from_job(job: &mut Job, non_inputs: &NonInputs) -> Result<(), RenderError> {
    if non_inputs.is_empty() {
        return Ok(());
    }
    let guard = guard_step(non_inputs)?;
    let mut steps = Vec::with_capacity(job.steps.len() + 1);
    for mut step in std::mem::take(&mut job.steps) {
        let repository_checkout = restrict_checkout(&mut step, non_inputs);
        steps.push(step);
        if repository_checkout {
            steps.push(guard.clone());
        }
    }
    job.steps = steps;
    Ok(())
}

fn restrict_checkout(step: &mut Step, non_inputs: &NonInputs) -> bool {
    let StepKind::Action { uses, with, .. } = &mut step.kind else {
        return false;
    };
    let foreign = ["repository", "path", SPARSE_CHECKOUT_KEY]
        .iter()
        .any(|key| with.contains_key(*key));
    if !uses.starts_with("actions/checkout@") || foreign {
        return false;
    }
    with.insert(
        SPARSE_CHECKOUT_KEY.to_owned(),
        non_inputs.sparse_checkout_patterns().join("\n"),
    );
    with.insert(SPARSE_CONE_MODE_KEY.to_owned(), "false".to_owned());
    true
}

pub(crate) fn guard_script(non_inputs: &NonInputs) -> String {
    let specs = non_inputs
        .pathspecs()
        .iter()
        .map(|spec| format!("'{spec}'"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        concat!(
            "set -eu; ",
            "mktemp \"${{RUNNER_TEMP:-${{TMPDIR:-/tmp}}}}/non-input-paths.XXXXXX\" | {{ ",
            "read -r list; trap 'rm -f \"$list\"' EXIT; ",
            "git ls-files -z -- {} > \"$list\"; ",
            "xargs -0 sh -c ",
            "'for p; do if [ -e \"$p\" ] || [ -L \"$p\" ]; then ",
            "printf \"non_input_present:%s\\n\" \"$p\" >&2; exit 1; fi; done' sh ",
            "< \"$list\"; exit; }}",
        ),
        specs
    )
}

fn guard_step(non_inputs: &NonInputs) -> Result<Step, RenderError> {
    let mut step = steps::shell_step(
        NON_INPUT_GUARD_STEP,
        vec!["sh".to_owned(), "-c".to_owned(), guard_script(non_inputs)],
        BTreeMap::new(),
    )?;
    step.role = Some(StepRole::NonInputGuard);
    Ok(step)
}

pub(crate) fn restrict_checkout_yaml(mut step: Yaml, non_inputs: &NonInputs) -> Vec<Yaml> {
    if non_inputs.is_empty() {
        return vec![step];
    }
    if let Yaml::Map(fields) = &mut step
        && let Some((_, Yaml::Map(inputs))) = fields.iter_mut().find(|(key, _)| key == "with")
    {
        inputs.push((
            SPARSE_CHECKOUT_KEY.to_owned(),
            Yaml::str(non_inputs.sparse_checkout_patterns().join("\n")),
        ));
        inputs.push((SPARSE_CONE_MODE_KEY.to_owned(), Yaml::Bool(false)));
    }
    let guard = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(NON_INPUT_GUARD_STEP)),
        ("run".to_owned(), Yaml::str(guard_script(non_inputs))),
    ]);
    vec![step, guard]
}
