// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const MBX_BUNDLE_PATH_EXPR: &str = "${{ runner.temp }}/velnor/mbx-bundle";

pub const MBX_BUNDLE_KEY_EXPR: &str = "${{steps.mbx-bundle-key.outputs.key}}";

pub const MBX_BUNDLE_RESTORE_KEYS_EXPR: &str = "${{steps.mbx-bundle-key.outputs.restore_keys}}";

pub const MBX_BUNDLE_KEY_OUTPUT: &str = "key";

pub const MBX_BUNDLE_RESTORE_KEYS_OUTPUT: &str = "restore_keys";

pub const MBX_BUNDLE_SAVE_OUTPUT: &str = "save";

pub const MBX_BUNDLE_EXPORTED_OUTPUT: &str = "exported";

pub const MBX_BUNDLE_IMPORT_CONDITION: &str =
    "steps.mbx-bundle-restore.outputs.cache-matched-key != ''";

pub const MBX_BUNDLE_EXPORT_CONDITION: &str = "always() && !cancelled() && steps.mbx-ready.outcome == 'success' && steps.mbx-bundle-key.outputs.save == 'true' && steps.mbx-bundle-restore.outputs.cache-hit != 'true'";

pub const MBX_BUNDLE_SAVE_CONDITION: &str =
    "always() && !cancelled() && steps.mbx-bundle-export.outputs.exported == 'true'";
