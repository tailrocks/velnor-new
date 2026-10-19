//! Fixed, credential-scrubbed preparation barrier for independent Cargo targets.

use crate::{RenderError, commands, steps};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{Step, canonical_json_str};

const SEED: &str = include_str!("../../../scripts/parallel-targets/seed.py");

/// Initialize missing Clippy/documentation lanes from a completed test build.
///
/// The shipped helper owns bounded copying and exclusive directory admission.
/// This fixed command is sequential and credential-scrubbed; it never changes
/// the subsequent check commands, parallel membership or target directories.
/// # Errors
/// Rejects unsafe workspace-relative target roots or noncanonical lane names.
pub fn seed_parallel_targets_step(target: &str, lanes: &[&str]) -> Result<Step, RenderError> {
    let parts: Vec<&str> = target.split('/').collect();
    if parts.last() != Some(&"target")
        || parts.iter().any(|part| {
            part.is_empty()
                || matches!(*part, "." | "..")
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        })
        || lanes.is_empty()
        || lanes.iter().copied().collect::<BTreeSet<_>>().len() != lanes.len()
        || lanes
            .iter()
            .any(|lane| !matches!(*lane, "velnor-clippy" | "velnor-doc"))
    {
        return Err(RenderError::BadCommand(
            "parallel_target_seed_arguments".to_owned(),
        ));
    }
    // JSON string quoting is also valid for this Python source string. Reuse
    // literal shell quoting so source text cannot acquire expansion semantics.
    let python = format!(
        "exec({})",
        canonical_json_str(SEED).map_err(RenderError::Contract)?
    );
    let mut command = format!(
        "exec python3 -I -c {} {}",
        commands::quote_script_arg(&python),
        target
    );
    for lane in lanes {
        command.push(' ');
        command.push_str(lane);
    }
    steps::shell_step(
        "Seed independent parallel targets",
        vec!["sh".to_owned(), "-c".to_owned(), command],
        BTreeMap::new(),
    )
}

#[cfg(test)]
#[path = "parallel_target_seed_tests.rs"]
mod tests;
