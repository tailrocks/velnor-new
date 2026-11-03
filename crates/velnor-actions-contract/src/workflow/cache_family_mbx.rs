// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const MBX_BUNDLE_PATH_EXPR: &str = "${{ runner.temp }}/velnor/mbx-bundle";

pub const MBX_BUNDLE_KEY_EXPR: &str = "${{steps.mbx-bundle-key.outputs.restore_key}}";

pub const MBX_BUNDLE_SAVE_KEY_EXPR: &str = "${{steps.mbx-bundle-export.outputs.key}}";

pub const MBX_BUNDLE_ROOT_ENV: &str = "VELNOR_MBX_BUNDLE_ROOT";

pub const MBX_BUNDLE_SNAPSHOT_ENV: &str = "VELNOR_STATE_SNAPSHOT";

pub const MBX_BUNDLE_PRODUCER_SKIP_OUTPUT: &str = "producer_skip";

pub const MBX_BUNDLE_MATCHED_KEY_EXPR: &str =
    "${{steps.mbx-bundle-restore.outputs.cache-matched-key}}";

pub const MBX_BUNDLE_MATCHED_KEY_ENV: &str = "VELNOR_MBX_BUNDLE_MATCHED_KEY";

pub const MBX_BUNDLE_KEY_OUTPUT: &str = "key";

pub const MBX_BUNDLE_RESTORE_KEY_OUTPUT: &str = "restore_key";

pub const MBX_BUNDLE_SAVE_OUTPUT: &str = "save";

pub const MBX_BUNDLE_EXPORTED_OUTPUT: &str = "exported";

pub const MBX_BUNDLE_RESTORE_CONDITION: &str = "steps.mbx-bundle-key.outputs.restore == 'true' && steps.mbx-bundle-key.outputs.restore_key != ''";

pub const MBX_BUNDLE_IMPORT_CONDITION: &str =
    "steps.mbx-bundle-restore.outputs.cache-matched-key != ''";

pub const MBX_BUNDLE_EXPORT_CONDITION: &str = "always() && !cancelled() && steps.mbx-ready.outcome == 'success' && steps.mbx-bundle-key.outputs.save == 'true'";

pub const MBX_BUNDLE_SAVE_CONDITION: &str =
    "always() && !cancelled() && steps.mbx-bundle-export.outputs.exported == 'true'";
