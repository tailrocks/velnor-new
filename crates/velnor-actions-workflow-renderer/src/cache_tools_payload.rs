//! Canonical read-only tool transport before the owned compiled bootstrap.
use crate::RenderError;
use std::collections::BTreeMap;
use velnor_actions_contract::{Job, StepKind, ToolCacheDomain};
#[path = "cache_mise_preflight.rs"]
pub(crate) mod preflight;

/// Restore the identical complete payload before any executable use.
pub(super) fn ensure_tool_payload(
    job: &mut Job,
    setup_at: usize,
    key: &str,
) -> Result<(), RenderError> {
    let expected = crate::cache_steps::tools_restore_step(key)?;
    let restores: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            crate::cache_tool_paths::transport_in_domain(step, ToolCacheDomain::Full)
                || step.name == crate::cache_steps::TOOLS_RESTORE_NAME
                || step
                    .id
                    .as_ref()
                    .is_some_and(|id| id.as_str() == crate::cache_steps::TOOLS_RESTORE_ID)
        })
        .map(|(at, _)| at)
        .collect();
    let restore_at = match restores.as_slice() {
        [] => {
            job.steps.insert(setup_at, platform_step()?);
            job.steps.insert(setup_at + 1, expected);
            setup_at + 1
        }
        [at] if *at + 1 == setup_at && job.steps[*at] == expected => *at,
        _ => return Err(invalid("tool_restore_changed_duplicate_or_misordered")),
    };
    for step in &mut job.steps {
        match &mut step.kind {
            StepKind::Shell { run, env } => {
                if super::detector_words(run).iter().any(|word| {
                    crate::cache_tool_paths::mise_word_domain(
                        word,
                        env.get("MISE_DATA_DIR").map(String::as_str),
                    )
                    .is_some_and(|domain| domain != ToolCacheDomain::Full)
                }) {
                    return Err(invalid("tool_shell_executable_domain_changed"));
                }
                env.insert(
                    "MISE_DATA_DIR".to_owned(),
                    ToolCacheDomain::Full.root().to_owned(),
                );
            }
            StepKind::SourceBoundHelper { env, .. }
                if env.contains_key("MISE_DATA_DIR")
                    && env.get("MISE_DATA_DIR").map(String::as_str)
                        != Some(ToolCacheDomain::Full.root()) =>
            {
                return Err(invalid("tool_helper_domain_changed"));
            }
            _ => {}
        }
    }
    let mut platform = platform_step()?;
    if let StepKind::Shell { env, .. } = &mut platform.kind {
        env.insert(
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::Full.root().to_owned(),
        );
    }
    let platforms: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.name == platform.name || step.kind == platform.kind)
        .collect();
    if !matches!(platforms.as_slice(), [(at, step)] if *at < restore_at && **step == platform) {
        return Err(invalid("tool_platform_changed_duplicate_or_misordered"));
    }
    Ok(())
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(reason.to_owned())
}

#[cfg(test)]
#[path = "cache_tools_payload_tests.rs"]
mod tests;

/// Bind actual hosted image identity; missing metadata fails closed.
pub(crate) fn platform_step() -> Result<velnor_actions_contract::Step, RenderError> {
    crate::steps::shell_step(
        "Resolve tool cache platform",
        vec!["sh".to_owned(), "-c".to_owned(),
            "set -eu; image=\"${ImageOS:?missing runner image}-${ImageVersion:?missing runner image version}\"; case \"$image\" in *[!a-zA-Z0-9._-]*) exit 1;; esac; printf 'VELNOR_CACHE_IMAGE=%s\\n' \"$image\" >> \"$GITHUB_ENV\"".to_owned()],
        BTreeMap::new(),
    )
}
