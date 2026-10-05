//! Explicit transport for the stock MBX action's local backend.
//!
//! The action owns setup and MBX's object format. This module owns only the
//! external bundle restore/import and export/save steps around producers.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use crate::{RenderError, cache_steps::cache_action_step, steps::shell_step};

/// Stable external bundle path shared by both official cache actions.
pub(crate) const MBX_BUNDLE_CACHE_PATH: &str = "${{ runner.temp }}/velnor/mbx-single-bundle";

const MBX_BUNDLE_SHELL_PATH: &str = "$RUNNER_TEMP/velnor/mbx-single-bundle";
const RESTORE_STEP_NAME: &str = "Restore MBX bundle";
const IMPORT_STEP_NAME: &str = "Import MBX bundle";
const EXPORT_STEP_NAME: &str = "Export MBX bundle";
const SAVE_STEP_NAME: &str = "Save MBX bundle";
const EMPTY_EXPORT_DIAGNOSTIC: &str = "no completed mbx builds are recorded for export group";

/// MBX bundle actions split around the producer that consumes the objects.
pub(crate) struct MbxBundleCacheSteps {
    /// Cache restore followed by MBX import, before compilation.
    pub(crate) restore: [Step; 2],
    /// MBX export followed by cache save, after successful compilation.
    pub(crate) save: [Step; 2],
}

/// Build the explicit stock-action bundle lifecycle.
///
/// Both cache actions receive the same key and path. The caller supplies the
/// validated trust-scoped key, its same-trust restore prefix, and the
/// protected-default writer condition.
/// # Errors
pub(crate) fn lifecycle_steps(
    restore_uses: &str,
    save_uses: &str,
    cache_key: &str,
    restore_prefix: &str,
    save_condition: &str,
) -> Result<MbxBundleCacheSteps, RenderError> {
    ensure_same_cache_pin(restore_uses, save_uses)?;

    let mut restore = cache_action_step(
        true,
        restore_uses,
        "mbx",
        cache_key,
        &[restore_prefix.to_owned()],
        &[MBX_BUNDLE_CACHE_PATH.to_owned()],
    )?;
    restore.name = RESTORE_STEP_NAME.to_owned();

    let import = import_step()?;

    let mut export = export_step()?;
    export.condition = Some(format!(
        "({save_condition}) && steps.mbx_cache_import.outputs.ready == 'true'"
    ));

    let mut save = cache_action_step(
        false,
        save_uses,
        "mbx",
        cache_key,
        &[],
        &[MBX_BUNDLE_CACHE_PATH.to_owned()],
    )?;
    save.name = SAVE_STEP_NAME.to_owned();
    save.condition = Some(format!(
        "({save_condition}) && steps.mbx_cache_export.outputs.ready == 'true'"
    ));

    Ok(MbxBundleCacheSteps {
        restore: [restore, import],
        save: [export, save],
    })
}

/// Reject restore/save action refs that would use different cache versions.
fn ensure_same_cache_pin(restore_uses: &str, save_uses: &str) -> Result<(), RenderError> {
    let restore_pin = restore_uses
        .rsplit_once('@')
        .map(|(_, pin)| pin)
        .ok_or_else(|| RenderError::BadActionRef("mbx_restore_ref_missing_pin".to_owned()))?;
    let save_pin = save_uses
        .rsplit_once('@')
        .map(|(_, pin)| pin)
        .ok_or_else(|| RenderError::BadActionRef("mbx_save_ref_missing_pin".to_owned()))?;
    if restore_pin != save_pin {
        return Err(RenderError::BadActionRef(
            "mbx_cache_action_pin_mismatch".to_owned(),
        ));
    }
    Ok(())
}

/// Restore the bundle into MBX's private local store, or continue cold.
fn import_step() -> Result<Step, RenderError> {
    let bundle_assignment = format!("bundle=\"{MBX_BUNDLE_SHELL_PATH}\"");
    let script = [
        "set -eu",
        "write_ready() { printf 'ready=%s\\n' \"$1\" >> \"$GITHUB_OUTPUT\"; }",
        "switch_to_cold_store() { printf 'MBX_CACHE_DIR=%s-cold\\n' \"${MBX_CACHE_DIR:?MBX_CACHE_DIR is required}\" >> \"$GITHUB_ENV\"; write_ready false; }",
        bundle_assignment.as_str(),
        "if [ -L \"$bundle\" ]; then printf '%s\\n' 'MBX bundle path is a symlink; using an empty private store' >&2; switch_to_cold_store; exit 0; fi",
        "if [ ! -e \"$bundle\" ]; then write_ready true; exit 0; fi",
        "if [ ! -d \"$bundle\" ]; then printf '%s\\n' 'MBX bundle path is not a directory; using an empty private store' >&2; switch_to_cold_store; exit 0; fi",
        "if mbx cache import \"$bundle\"; then write_ready true; else printf '%s\\n' 'MBX bundle import failed; using an empty private store' >&2; switch_to_cold_store; fi",
    ]
    .join("; ");
    shell_step(
        IMPORT_STEP_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )
}

/// Export useful objects only in the trusted, successful producer scope.
fn export_step() -> Result<Step, RenderError> {
    let bundle_assignment = format!("bundle=\"{MBX_BUNDLE_SHELL_PATH}\"");
    let export_command = format!(
        "if mbx cache export --group \"$group\" --format directory \"$bundle\" > \"$log\" 2>&1; then cat \"$log\"; else status=$?; if grep -Fq '{EMPTY_EXPORT_DIAGNOSTIC}' \"$log\"; then cat \"$log\"; write_ready false; exit 0; fi; cat \"$log\" >&2; exit \"$status\"; fi"
    );
    let script = [
        "set -eu",
        "write_ready() { printf 'ready=%s\\n' \"$1\" >> \"$GITHUB_OUTPUT\"; }",
        "write_ready false",
        bundle_assignment.as_str(),
        "mkdir -p \"$RUNNER_TEMP/velnor\"",
        "has_entries() { for entry in \"$bundle\"/* \"$bundle\"/.[!.]* \"$bundle\"/..?*; do if [ -e \"$entry\" ] || [ -L \"$entry\" ]; then return 0; fi; done; return 1; }",
        "if [ -L \"$bundle\" ]; then printf '%s\\n' 'MBX bundle path is a symlink; refusing export' >&2; exit 1; fi",
        "if [ -e \"$bundle\" ]; then if [ ! -d \"$bundle\" ]; then printf '%s\\n' 'MBX bundle path is not a directory; refusing export' >&2; exit 1; fi; if has_entries; then printf '%s\\n' 'MBX bundle path is not empty; refusing export' >&2; exit 1; fi; rmdir \"$bundle\"; fi",
        "group=\"${MBX_CACHE_EXPORT_GROUP:?MBX_CACHE_EXPORT_GROUP is required}\"",
        "log=\"$RUNNER_TEMP/velnor/mbx-cache-export.log\"",
        "if [ -L \"$log\" ]; then printf '%s\\n' 'MBX export log path is a symlink; refusing export' >&2; exit 1; fi",
        export_command.as_str(),
        "if [ -L \"$bundle\" ] || [ ! -d \"$bundle\" ]; then printf '%s\\n' 'MBX export did not create a bundle directory' >&2; exit 1; fi",
        "if has_entries; then write_ready true; else printf '%s\\n' 'MBX export produced an empty bundle'; fi",
    ]
    .join("; ");
    shell_step(
        EXPORT_STEP_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::StepKind;

    use super::*;
    use crate::cache_steps::{TOOLS_RESTORE_USES, TOOLS_SAVE_USES};

    const KEY: &str = "velnor-v1-mbx-trusted-linux-x64-rust-1.98.1-mbx-1.21.1-directory-clippy-b3-compat-${{ hashFiles('Cargo.lock') }}";
    const PREFIX: &str =
        "velnor-v1-mbx-trusted-linux-x64-rust-1.98.1-mbx-1.21.1-directory-clippy-b3-compat-";
    const SAVE_CONDITION: &str =
        "success() && github.event_name == 'push' && github.ref_protected == true";

    fn steps() -> Result<MbxBundleCacheSteps, RenderError> {
        lifecycle_steps(
            TOOLS_RESTORE_USES,
            TOOLS_SAVE_USES,
            KEY,
            PREFIX,
            SAVE_CONDITION,
        )
    }

    fn action_data(step: &Step) -> Option<(&str, &BTreeMap<String, String>)> {
        match &step.kind {
            StepKind::Action { uses, with, .. } => Some((uses, with)),
            _ => None,
        }
    }

    fn shell_text(step: &Step) -> Option<String> {
        match &step.kind {
            StepKind::Shell { run, .. } => Some(run.join(" ")),
            _ => None,
        }
    }

    #[test]
    fn restore_and_save_share_key_path_and_action_pin() -> Result<(), RenderError> {
        let lifecycle = steps()?;
        let restore = action_data(&lifecycle.restore[0])
            .ok_or_else(|| RenderError::InvalidWorkflow("expected_restore_action".to_owned()))?;
        let save = action_data(&lifecycle.save[1])
            .ok_or_else(|| RenderError::InvalidWorkflow("expected_save_action".to_owned()))?;
        assert_eq!(lifecycle.restore[0].name, RESTORE_STEP_NAME);
        assert_eq!(lifecycle.save[1].name, SAVE_STEP_NAME);
        assert_eq!(restore.0, TOOLS_RESTORE_USES);
        assert_eq!(save.0, TOOLS_SAVE_USES);
        assert_eq!(restore.1.get("key").map(String::as_str), Some(KEY));
        assert_eq!(save.1.get("key").map(String::as_str), Some(KEY));
        assert_eq!(
            restore.1.get("path").map(String::as_str),
            Some(MBX_BUNDLE_CACHE_PATH)
        );
        assert_eq!(
            save.1.get("path").map(String::as_str),
            Some(MBX_BUNDLE_CACHE_PATH)
        );
        assert_eq!(
            restore.1.get("restore-keys").map(String::as_str),
            Some(PREFIX)
        );
        Ok(())
    }

    #[test]
    fn import_uses_the_external_bundle_and_switches_cold_after_failure() -> Result<(), RenderError>
    {
        let lifecycle = steps()?;
        let import = shell_text(&lifecycle.restore[1])
            .ok_or_else(|| RenderError::InvalidWorkflow("expected_import_shell".to_owned()))?;
        assert_eq!(lifecycle.restore[1].name, IMPORT_STEP_NAME);
        assert!(import.contains("bundle=\"$RUNNER_TEMP/velnor/mbx-single-bundle\""));
        assert!(import.contains("mbx cache import \"$bundle\""));
        assert!(import.contains("MBX_CACHE_DIR=%s-cold"));
        assert!(import.contains("write_ready false"));
        assert!(import.contains("write_ready true"));
        assert!(!import.contains("rm -rf"));
        Ok(())
    }

    #[test]
    fn export_is_success_gated_and_only_saves_a_nonempty_bundle() -> Result<(), RenderError> {
        let lifecycle = steps()?;
        let export = shell_text(&lifecycle.save[0])
            .ok_or_else(|| RenderError::InvalidWorkflow("expected_export_shell".to_owned()))?;
        assert_eq!(lifecycle.save[0].name, EXPORT_STEP_NAME);
        assert_eq!(
            lifecycle.save[0].condition.as_deref(),
            Some(
                "(success() && github.event_name == 'push' && github.ref_protected == true) && steps.mbx_cache_import.outputs.ready == 'true'"
            )
        );
        assert!(
            export.contains("mbx cache export --group \"$group\" --format directory \"$bundle\"")
        );
        assert!(export.contains(EMPTY_EXPORT_DIAGNOSTIC));
        assert!(export.contains("write_ready false"));
        assert!(export.contains("write_ready true"));
        assert!(export.contains("has_entries()"));
        assert_eq!(
            lifecycle.save[1].condition.as_deref(),
            Some(
                "(success() && github.event_name == 'push' && github.ref_protected == true) && steps.mbx_cache_export.outputs.ready == 'true'"
            )
        );
        Ok(())
    }

    #[test]
    fn restore_and_save_must_use_the_same_cache_action_pin() {
        let result = lifecycle_steps(
            TOOLS_RESTORE_USES,
            "actions/cache/save@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            KEY,
            PREFIX,
            SAVE_CONDITION,
        );
        assert!(matches!(
            result,
            Err(RenderError::BadActionRef(problem))
                if problem == "mbx_cache_action_pin_mismatch"
        ));
    }
}
