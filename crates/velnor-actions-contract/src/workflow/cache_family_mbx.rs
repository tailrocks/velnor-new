// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const MBX_BUNDLE_PATH_EXPR: &str = "${{ runner.temp }}/velnor/mbx-bundle";

pub const MBX_BUNDLE_KEY_EXPR: &str = "${{steps.mbx-bundle-key.outputs.key}}";

pub const MBX_BUNDLE_RESTORE_KEYS_EXPR: &str = "${{steps.mbx-bundle-key.outputs.restore_keys}}";

pub const MBX_BUNDLE_KEY_OUTPUT: &str = "key";

pub const MBX_BUNDLE_RESTORE_KEYS_OUTPUT: &str = "restore_keys";

pub const MBX_BUNDLE_SAVE_OUTPUT: &str = "save";

/// Job-output names that publish the reader branch key computation for the
/// writer branch recheck. The writer recheck runs before the writer's own
/// key step, so it cannot read `steps.mbx-bundle-key.outputs.*` (a later
/// step); it reads the reader's published outputs through `needs` instead.
pub const MBX_BUNDLE_KEY_JOB_OUTPUT: &str = "mbx_bundle_key";

pub const MBX_BUNDLE_RESTORE_KEYS_JOB_OUTPUT: &str = "mbx_bundle_restore_keys";

/// `needs.<reader>.outputs.mbx_bundle_key` for the writer recheck restore.
#[must_use]
pub fn mbx_bundle_key_needs_expr(reader_job: &str) -> String {
    format!("${{{{ needs.{reader_job}.outputs.{MBX_BUNDLE_KEY_JOB_OUTPUT} }}}}")
}

/// `needs.<reader>.outputs.mbx_bundle_restore_keys` for the writer recheck.
#[must_use]
pub fn mbx_bundle_restore_keys_needs_expr(reader_job: &str) -> String {
    format!("${{{{ needs.{reader_job}.outputs.{MBX_BUNDLE_RESTORE_KEYS_JOB_OUTPUT} }}}}")
}

/// Step `if` guard so the writer recheck only restores when the reader
/// published a key. The reader is skipped on writer events, leaving its
/// outputs empty; restoring with an empty key would fail the writer.
#[must_use]
pub fn mbx_bundle_recheck_condition(reader_job: &str) -> String {
    format!("needs.{reader_job}.outputs.{MBX_BUNDLE_KEY_JOB_OUTPUT} != ''")
}

/// Match an already-extracted `${{ }}` inner expression against
/// `needs.<job>.outputs.<output>` (`expression_spans` strips the braces).
fn mbx_bundle_needs_output(inner: &str, output: &str) -> bool {
    let Some(rest) = inner.strip_prefix("needs.") else {
        return false;
    };
    let mut parts = rest.split('.');
    matches!(
        (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next()
        ),
        (Some(job), Some("outputs"), Some(name), None)
            if name == output
                && !job.is_empty()
                && job
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
    )
}

/// Whether full `with` text is exactly one bundle reader reference.
fn is_mbx_bundle_needs_text(text: &str, output: &str) -> bool {
    let Some(inner) = text
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .map(str::trim)
    else {
        return false;
    };
    mbx_bundle_needs_output(inner, output)
}

/// Whether `with` text is exactly `needs.<job>.outputs.mbx_bundle_key`.
#[must_use]
pub fn is_mbx_bundle_key_needs_expr(text: &str) -> bool {
    is_mbx_bundle_needs_text(text, MBX_BUNDLE_KEY_JOB_OUTPUT)
}

/// Whether `with` text is exactly
/// `needs.<job>.outputs.mbx_bundle_restore_keys`.
#[must_use]
pub fn is_mbx_bundle_restore_keys_needs_expr(text: &str) -> bool {
    is_mbx_bundle_needs_text(text, MBX_BUNDLE_RESTORE_KEYS_JOB_OUTPUT)
}

/// Whether an extracted `${{ }}` inner span is
/// `needs.<job>.outputs.mbx_bundle_key`.
#[must_use]
pub fn is_mbx_bundle_key_needs_inner(inner: &str) -> bool {
    mbx_bundle_needs_output(inner, MBX_BUNDLE_KEY_JOB_OUTPUT)
}

/// Whether an extracted `${{ }}` inner span is
/// `needs.<job>.outputs.mbx_bundle_restore_keys`.
#[must_use]
pub fn is_mbx_bundle_restore_keys_needs_inner(inner: &str) -> bool {
    mbx_bundle_needs_output(inner, MBX_BUNDLE_RESTORE_KEYS_JOB_OUTPUT)
}

pub const MBX_BUNDLE_EXPORTED_OUTPUT: &str = "exported";

pub const MBX_BUNDLE_IMPORT_CONDITION: &str =
    "steps.mbx-bundle-restore.outputs.cache-matched-key != ''";

pub const MBX_BUNDLE_EXPORT_CONDITION: &str = "always() && !cancelled() && steps.mbx-ready.outcome == 'success' && steps.mbx-bundle-key.outputs.save == 'true' && steps.mbx-bundle-restore.outputs.cache-hit != 'true'";

pub const MBX_BUNDLE_SAVE_CONDITION: &str =
    "always() && !cancelled() && steps.mbx-bundle-export.outputs.exported == 'true'";
