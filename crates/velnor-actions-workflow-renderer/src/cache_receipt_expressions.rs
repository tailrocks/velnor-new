//! Receipt observations use fixed bindings; recipe admission seals the actual key.
use crate::RenderError;

pub(super) fn check(key: &str, value: &str, spans: &[&str]) -> Option<Result<(), RenderError>> {
    let expected = match key {
        "VELNOR_CACHE_SOURCE_REPOSITORY" => "${{ github.repository }}",
        "VELNOR_CACHE_SOURCE_REPOSITORY_ID" => "${{ github.repository_id }}",
        "VELNOR_CACHE_SOURCE_SHA" => "${{ github.sha }}",
        "VELNOR_CACHE_RUN_ID" => "${{ github.run_id }}",
        "VELNOR_CACHE_RUN_ATTEMPT" => "${{ github.run_attempt }}",
        "VELNOR_CACHE_RECEIPT_BUNDLE_PATH" => {
            "${{ steps.velnor-cache-receipt-attest.outputs.bundle-path }}"
        }
        "VELNOR_CACHE_PAYLOAD_ROOT" => "${{ runner.temp }}/velnor",
        "VELNOR_CACHE_RECEIPT_ROOT" => return Some(result(key, receipt_root(value, spans))),
        "VELNOR_CACHE_RECEIPT_KEY" => return Some(result(key, receipt_key(value, spans))),
        _ => return None,
    };
    Some(result(key, value == expected))
}

fn result(key: &str, allowed: bool) -> Result<(), RenderError> {
    if allowed {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_env_expression:{key}")))
    }
}

fn receipt_root(value: &str, spans: &[&str]) -> bool {
    value
        .strip_prefix("${{ runner.temp }}/velnor/cache-receipts/")
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
        && spans == ["runner.temp"]
}

fn receipt_key(value: &str, spans: &[&str]) -> bool {
    if super::source_report::publication_key(value, spans).is_ok() {
        return true;
    }
    let suffix = "-${{env.VELNOR_CACHE_IMAGE}}-snapshot-${{steps.velnor-tool-after.outputs.digest}}-${{github.run_id}}-${{github.run_attempt}}";
    value.strip_suffix(suffix).is_some_and(|prefix| {
        !prefix.is_empty()
            && prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    }) && spans
        == [
            "env.VELNOR_CACHE_IMAGE",
            "steps.velnor-tool-after.outputs.digest",
            "github.run_id",
            "github.run_attempt",
        ]
}
