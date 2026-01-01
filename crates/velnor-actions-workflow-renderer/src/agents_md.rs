//! Generated `.github/AGENTS.md` rendering.

use velnor_actions_contract::AGENTS_MD_PATH;

use crate::RenderError;
use crate::marker::with_marker;
use crate::render::RenderedFile;

const AGENTS_MD_BODY: &str = "\
# Generated GitHub Workflows and Configurations

`velnor-actions` from [tailrocks/velnor-new](https://github.com/tailrocks/velnor-new) owns its declared generated paths, the complete `.github/workflows/` namespace, and marked shared action definitions under `.github/actions/`. Successful generation replaces emitted owned paths and retires owned paths no longer emitted. Do not hand-edit these managed paths; change their inputs and run `velnor-actions generate`.

Other existing files and directories under `.github/` are repository-owned and preserved by generation, including empty directories. A directory emptied solely by retiring its managed generated outputs may be pruned. Put custom files outside `.github/workflows/`, which is reserved for generated workflows. Generation refuses to overwrite an unmarked custom action when its path collides with a generated shared action.

## Generation Inputs

- **Keep generation inputs outside `.github/`**: Repository configuration belongs in `.velnor/config.toml`, workspace manifests, and toolchain settings outside `.github/`.
- **Regenerate via CLI**: Apply configuration changes by running `velnor-actions generate`.

## Workflow Issues and Enhancements

When issues, bugs, limitations, or enhancements are observed in generated GitHub Actions workflows:

1. **Do NOT patch generator-owned outputs or create repo-specific workflow workarounds**:
   - Avoid local patches, overrides, or shims to managed workflow, config, or action files.
2. **Research generic solutions in `velnor-actions` / `velnor-new`**:
   - Investigate solutions in the upstream generator: [tailrocks/velnor-new](https://github.com/tailrocks/velnor-new).
   - Verify that potential changes align with the vision and architectural invariants of Velnor Actions (generic workflow generator, strict safety invariants, zero legacy, fast CI).
3. **Analyze and verify with subagents**:
   - Compare proposed designs against the current implementation in `velnor-new` using detailed subagent analysis.
   - Use multiple subagents to review the concept, implementation details, and edge cases.
4. **Submit PR to upstream `velnor-new`**:
   - Only after thorough verification and review, submit a PR to `velnor-new` (https://github.com/tailrocks/velnor-new).
5. **Regenerate workflows**:
   - After the fix is merged in `velnor-new`, update/regenerate the workflows using `velnor-actions generate`.

## Root Repository Rules

- If a root `AGENTS.md` is present in this repository, all of its rules, invariants, and guidelines apply here as well.
- In case of conflict, stricter safety, correctness, and verification standards take precedence.
";

/// Render the default `.github/AGENTS.md` file with the versioned marker.
///
/// # Errors
///
/// Returns [`RenderError`] if the version is invalid.
pub fn render_agents_md(version: &str) -> Result<RenderedFile, RenderError> {
    let bytes = with_marker(version, AGENTS_MD_BODY)?;
    Ok(RenderedFile {
        path: AGENTS_MD_PATH.to_owned(),
        bytes,
    })
}
