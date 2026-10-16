//! Generated-tree freshness step constructors for `import` and `symlink` modes.

use std::collections::BTreeMap;

use velnor_actions_contract::{ClaudeMdMode, Step, StepRole};

use crate::{
    RenderError,
    closure::CHECK_GENERATED_NAME,
    closure_paths::{validate_helper_path, validate_output_dir},
    steps,
};

/// Fixed freshness step: regenerate into scratch, byte-compare `.github`.
///
/// Runs the helper's `generate --output-dir`, diffs preview against
/// committed `.github`; `env` is the caller-validated consumer env,
/// scrubbed here since local generation needs no ambient auth.
/// # Errors
pub fn freshness_step(
    binary: &str,
    output_dir: &str,
    env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    validate_helper_path(binary)?;
    validate_output_dir(output_dir)?;
    let script = format!(
        "{binary} generate --output-dir \"{output_dir}\" && diff -r --brief .github \"{output_dir}/.github\""
    );
    let argv = vec!["sh".to_owned(), "-c".to_owned(), script];
    let mut step = steps::shell_step(CHECK_GENERATED_NAME, argv, env.clone())?;
    step.role = Some(StepRole::CheckGenerated);
    Ok(step)
}

/// Freshness step parameterized by the repository's `.github/CLAUDE.md` emission mode.
///
/// In `import` mode this emits the exact [`freshness_step`] command; in `symlink`
/// mode it additionally verifies `.github/CLAUDE.md -> AGENTS.md` in both the
/// committed tree and the scratch output before running `diff -r --brief`.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid helper path, output directory, or env.
pub fn freshness_step_with_claude_mode(
    binary: &str,
    output_dir: &str,
    env: &BTreeMap<String, String>,
    claude_md: ClaudeMdMode,
) -> Result<Step, RenderError> {
    match claude_md {
        ClaudeMdMode::Import => freshness_step(binary, output_dir, env),
        ClaudeMdMode::Symlink => {
            validate_helper_path(binary)?;
            validate_output_dir(output_dir)?;
            let script = format!(
                "{binary} generate --output-dir \"{output_dir}\" && test -L .github/CLAUDE.md && test -L \"{output_dir}/.github/CLAUDE.md\" && readlink .github/CLAUDE.md | grep -qxF AGENTS.md && readlink \"{output_dir}/.github/CLAUDE.md\" | grep -qxF AGENTS.md && diff -r --brief .github \"{output_dir}/.github\""
            );
            let argv = vec!["sh".to_owned(), "-c".to_owned(), script];
            let mut step = steps::shell_step(CHECK_GENERATED_NAME, argv, env.clone())?;
            step.role = Some(StepRole::CheckGenerated);
            Ok(step)
        }
    }
}
