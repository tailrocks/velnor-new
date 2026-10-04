//! Source-owned removal of canonical root leaves before receipt materialization.

use velnor_actions_contract::ToolCacheDomain;

/// Fixed selected domain footprint; executable/source siblings are not inferred.
#[must_use]
pub(super) fn source(domain: ToolCacheDomain) -> String {
    let roots: Vec<_> = domain
        .payload()
        .into_iter()
        .map(|root| root.replace("${{ runner.temp }}/velnor/", ""))
        .collect();
    let code = format!(
        "exec({COMMON:?}); exec({SOURCE:?}); clear_selected_roots(os.environ[\"RUNNER_TEMP\"], {roots:?})"
    );
    let code = code.replace('\'', "'\\''");
    format!("/usr/bin/python3 -I -S -c '{code}'")
}

const COMMON: &str = include_str!("catalog_native_health.py");
const SOURCE: &str = include_str!("catalog_native_root_clear.py");
