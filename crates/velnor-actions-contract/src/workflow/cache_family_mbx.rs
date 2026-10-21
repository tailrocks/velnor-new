//! MBX bundle family constants: path, key outputs, and the gating conditions.

/// Directory `mbx cache export --format directory` writes and `mbx cache import` consumes.
pub const MBX_BUNDLE_PATH_EXPR: &str = "${{ runner.temp }}/velnor/mbx-bundle";
/// Exact key of the MBX bundle, from the job's own `Compute MBX bundle key` step.
pub const MBX_BUNDLE_KEY_EXPR: &str = "${{steps.mbx-bundle-key.outputs.key}}";
/// Restore keys of the MBX bundle (same compatibility, any snapshot).
pub const MBX_BUNDLE_RESTORE_KEYS_EXPR: &str = "${{steps.mbx-bundle-key.outputs.restore_keys}}";
/// Key-step output: exact bundle key.
pub const MBX_BUNDLE_KEY_OUTPUT: &str = "key";
/// Key-step output: newline-separated bundle restore keys.
pub const MBX_BUNDLE_RESTORE_KEYS_OUTPUT: &str = "restore_keys";
/// Key-step output: `true` only on a trusted protected push.
pub const MBX_BUNDLE_SAVE_OUTPUT: &str = "save";
/// Export-step output: `true` only when a bundle within the ceiling was exported.
pub const MBX_BUNDLE_EXPORTED_OUTPUT: &str = "exported";
/// Import runs only for an entry the restore step actually matched.
pub const MBX_BUNDLE_IMPORT_CONDITION: &str =
    "steps.mbx-bundle-restore.outputs.cache-matched-key != ''";
/// Export after the producer, whatever later tests, lints or docs did (decision 1).
///
/// The producer is checked inside the export script through the marker the
/// `Record MBX producer success` step writes right after the compile, so the
/// same gate holds when the job body is a shared composite. Trusted pushes
/// only, and never when the restore already holds the exact snapshot.
pub const MBX_BUNDLE_EXPORT_CONDITION: &str = "always() && !cancelled() && steps.mbx-ready.outcome == 'success' && steps.mbx-bundle-key.outputs.save == 'true' && steps.mbx-bundle-restore.outputs.cache-hit != 'true'";
/// Save only a bundle the export step admitted.
pub const MBX_BUNDLE_SAVE_CONDITION: &str =
    "always() && !cancelled() && steps.mbx-bundle-export.outputs.exported == 'true'";
