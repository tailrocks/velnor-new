// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepKind, StepRole};

const HIT: &str = "${{ steps.sources-restore.outputs.cache-hit }}";
const KEY: &str = "${{ steps.sources-restore.outputs.cache-matched-key }}";

pub(crate) fn step(env: BTreeMap<String, String>) -> Result<Step, RenderError> {
    let mut step = crate::shell_step(
        "Sanitize and prefetch Cargo sources",
        vec![
            "bash".into(),
            "-c".into(),
            crate::verification_jobs::mise_task_bootstrap::runner_family_prefetch_script(),
        ],
        env,
    )?;
    let StepKind::Shell { env, .. } = &mut step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "runner_family_prefetch_kind".into(),
        ));
    };
    env.insert("SOURCES_CACHE_HIT".into(), HIT.into());
    env.insert("SOURCES_CACHE_MATCHED_KEY".into(), KEY.into());
    step.role = Some(StepRole::RunnerFamilySourcesPrefetch);
    Ok(step)
}

pub(crate) fn validate(run: &[String], env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    let expected = [
        "bash".to_owned(),
        "-c".to_owned(),
        crate::toolchain_env::with_credential_unset_script(
            &crate::verification_jobs::mise_task_bootstrap::runner_family_prefetch_script(),
        ),
    ];
    if run != expected
        || env.get("SOURCES_CACHE_HIT").map(String::as_str) != Some(HIT)
        || env.get("SOURCES_CACHE_MATCHED_KEY").map(String::as_str) != Some(KEY)
        || env.get("VELNOR_PREFETCH_RUST_ONLY").map(String::as_str) != Some("1")
    {
        return Err(RenderError::InvalidWorkflow(
            "runner_family_prefetch_binding".into(),
        ));
    }
    let mut other = env.clone();
    other.remove("SOURCES_CACHE_HIT");
    other.remove("SOURCES_CACHE_MATCHED_KEY");
    crate::commands::validate_env(&other)
}

#[cfg(test)]
mod tests {
    use super::{step, validate};
    use std::{collections::BTreeMap, error::Error};
    use velnor_actions_contract::StepKind;

    #[test]
    fn prefetch_accepts_only_its_bound_cache_outputs() -> Result<(), Box<dyn Error>> {
        let step = step(BTreeMap::from([(
            "VELNOR_PREFETCH_RUST_ONLY".into(),
            "1".into(),
        )]))?;
        let StepKind::Shell { run, env } = step.kind else {
            return Err("expected shell".into());
        };
        validate(&run, &env)?;
        for (hit, matched) in [
            (
                "${{ steps.sources-restore.outputs.cache-matched-key }}",
                "${{ steps.sources-restore.outputs.cache-hit }}",
            ),
            (
                "${{ github.sha }}",
                "${{ steps.sources-restore.outputs.cache-matched-key }}",
            ),
        ] {
            let mut changed = env.clone();
            changed.insert("SOURCES_CACHE_HIT".into(), hit.into());
            changed.insert("SOURCES_CACHE_MATCHED_KEY".into(), matched.into());
            assert!(
                validate(&run, &changed).is_err(),
                "accepted unbound cache output"
            );
        }
        let mut changed_run = run.clone();
        changed_run[2].push_str("; true");
        assert!(
            validate(&changed_run, &env).is_err(),
            "accepted changed prefetch command"
        );
        Ok(())
    }
}
