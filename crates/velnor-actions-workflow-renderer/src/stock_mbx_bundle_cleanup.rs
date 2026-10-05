use velnor_actions_contract::Step;

use crate::{RenderError, steps::shell_step};

use super::{CHILD_VALIDATION, VALIDATION};

const NAME: &str = "Cleanup MBX single bundle";

pub(super) fn cleanup_step() -> Result<Step, RenderError> {
    let mut lines = vec!["set -eu".to_owned()];
    lines.extend(VALIDATION.map(str::to_owned));
    lines.extend(CHILD_VALIDATION.map(str::to_owned));
    lines.extend(
        [
            "bundle_identity=\"${VELNOR_MBX_EXPORTED_BUNDLE_IDENTITY:-${VELNOR_MBX_IMPORTED_BUNDLE_IDENTITY:-}}\"",
            "[ \"$VELNOR_MBX_EXPORT_OUTCOME\" != cancelled ] && [ \"$VELNOR_MBX_EXPORT_OUTCOME\" != failure ] || [ \"$VELNOR_MBX_EXPORT_CLEANUP_SAFE\" = true ] || fail 'MBX export did not prove its child stopped'",
            "[ \"$VELNOR_MBX_IMPORT_OUTCOME\" != cancelled ] && [ \"$VELNOR_MBX_IMPORT_OUTCOME\" != failure ] || [ \"$VELNOR_MBX_IMPORT_QUIESCED\" = true ] || fail 'MBX import did not prove its child stopped'",
            "[ \"$VELNOR_MBX_EXPORT_OUTCOME\" != failure ] || ! bundle_exists || fail 'unsafe MBX export failure blocks cleanup'",
            "[ \"$VELNOR_MBX_IMPORT_OUTCOME\" != failure ] || ! bundle_exists || fail 'unsafe MBX import failure blocks cleanup'",
            "if bundle_exists; then if [ -n \"$bundle_identity\" ]; then validate_owned_bundle \"$bundle\" \"$bundle_identity\" || fail 'workspace bundle identity changed'; elif { [ \"$VELNOR_MBX_RESTORE_OUTCOME\" = cancelled ] || [ \"$VELNOR_MBX_RESTORE_OUTCOME\" = skipped ]; } && [ \"$VELNOR_MBX_IMPORT_OUTCOME\" = skipped ] && [ \"$VELNOR_MBX_EXPORT_OUTCOME\" = skipped ]; then bundle_identity=\"$VELNOR_MBX_BUNDLE_IDENTITY\"; validate_owned_bundle \"$bundle\" \"$bundle_identity\" || fail 'preflight bundle identity changed before restore'; else fail 'workspace bundle cleanup identity is unavailable'; fi; safe_clear_contents \"$bundle\" \"$bundle_identity\"; rmdir \"$bundle\" || fail 'cannot remove empty workspace bundle'; fi",
            "if [ \"$VELNOR_MBX_IMPORT_OUTCOME\" = cancelled ] || [ \"$VELNOR_MBX_IMPORT_OUTCOME\" = failure ]; then safe_clear_contents \"$primary\" \"$VELNOR_MBX_PRIMARY_IDENTITY\"; safe_clear_contents \"$cold\" \"$VELNOR_MBX_COLD_IDENTITY\"; fi",
            "[ ! -e \"$bundle\" ] && [ ! -L \"$bundle\" ] || fail 'workspace bundle remains after cleanup'",
            "workspace_path_is_owned && validate_private \"$transport_parent\" \"$VELNOR_MBX_TRANSPORT_PARENT_IDENTITY\" && rmdir \"$transport_parent\" || fail 'cannot remove empty MBX transport parent'",
            "workspace_path_is_owned && validate_private \"$lease\" \"$VELNOR_MBX_TRANSPORT_LEASE_IDENTITY\" && rmdir \"$lease\" || fail 'cannot release MBX workspace lease'",
        ]
        .map(str::to_owned),
    );
    let mut env = super::preflight_env();
    env.insert(
        "MBX_CACHE_EXPORT_GROUP".to_owned(),
        "${{ steps.mbx_cache_preflight.outputs.export_group }}".to_owned(),
    );
    env.insert(
        "VELNOR_MBX_EXPORTED_BUNDLE_IDENTITY".to_owned(),
        "${{ steps.mbx_cache_export.outputs.bundle_identity }}".to_owned(),
    );
    env.insert(
        "VELNOR_MBX_IMPORTED_BUNDLE_IDENTITY".to_owned(),
        "${{ steps.mbx_cache_import.outputs.bundle_identity }}".to_owned(),
    );
    env.insert(
        "VELNOR_MBX_EXPORT_OUTCOME".to_owned(),
        "${{ steps.mbx_cache_export.outcome }}".to_owned(),
    );
    env.insert(
        "VELNOR_MBX_EXPORT_CLEANUP_SAFE".to_owned(),
        "${{ steps.mbx_cache_export.outputs.cleanup_safe }}".to_owned(),
    );
    env.insert(
        "VELNOR_MBX_IMPORT_OUTCOME".to_owned(),
        "${{ steps.mbx_cache_import.outcome }}".to_owned(),
    );
    env.insert(
        "VELNOR_MBX_IMPORT_QUIESCED".to_owned(),
        "${{ steps.mbx_cache_import.outputs.mbx_quiesced }}".to_owned(),
    );
    env.insert(
        "VELNOR_MBX_RESTORE_OUTCOME".to_owned(),
        "${{ steps.mbx_cache_restore.outcome }}".to_owned(),
    );
    let mut step = shell_step(
        NAME,
        vec!["sh".to_owned(), "-c".to_owned(), lines.join("; ")],
        env,
    )?;
    step.condition = Some("always() && steps.mbx_cache_preflight.outcome == 'success'".to_owned());
    Ok(step)
}
