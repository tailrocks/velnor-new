// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepRole, validate_fetch_root};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use super::PlanSourcesError;

pub const FETCH_SOURCES_STEP: &str = "Fetch Cargo sources";
pub const CARGO_CLEAN_DIR: &str = "$RUNNER_TEMP/velnor/cargo-clean";

#[must_use]
pub fn cargo_isolation_prefix() -> String {
    format!("mkdir -p \"{CARGO_CLEAN_DIR}\" && cd \"{CARGO_CLEAN_DIR}\" && ")
}

#[must_use]
pub fn isolated_manifest_flag(root: &str) -> String {
    format!(
        "--manifest-path \"$GITHUB_WORKSPACE/{}\"",
        workspace_manifest(root)
    )
}

pub fn fetch_steps_with(
    catalog: &ToolCatalog,
    roots: &[String],
    env: &BTreeMap<String, String>,
    remove_credentials: bool,
    all_features: bool,
) -> Result<Vec<Step>, PlanSourcesError> {
    let mut steps = Vec::with_capacity(roots.len());
    for root in roots {
        validate_fetch_root(root).map_err(PlanSourcesError::Contract)?;
        let mut script = fetch_script(catalog, root, all_features);
        if remove_credentials {
            let credentials = crate::toolchain_env::credential_scrub()
                .into_keys()
                .collect::<Vec<_>>()
                .join(" ");
            script = format!("unset {credentials}; {script}");
        }
        let manifest = workspace_manifest(root);
        let name = if root.is_empty() {
            FETCH_SOURCES_STEP.to_owned()
        } else {
            format!("{FETCH_SOURCES_STEP} ({manifest})")
        };
        let mut step = crate::steps::ambient_shell_step(
            &name,
            vec!["sh".to_owned(), "-c".to_owned(), script],
            env.clone(),
        )
        .map_err(PlanSourcesError::ShellRender)?;
        step.role = Some(StepRole::CargoSourcesFetch);
        steps.push(step);
    }
    Ok(steps)
}

fn fetch_script(catalog: &ToolCatalog, root: &str, all_features: bool) -> String {
    let spec = catalog.tool_spec(PinnedTool::Rust);
    let base = format!("mise --no-config --no-env --no-hooks exec {spec} -- cargo");
    let manifest = format!(" {}", isolated_manifest_flag(root));
    let features = if all_features { " --all-features" } else { "" };
    let probe = format!("{base} metadata --locked --offline{features}{manifest} >/dev/null 2>&1");
    let log = "\"$RUNNER_TEMP/velnor/cargo-clean/fetch-stderr.log\"";
    let count = "\"$RUNNER_TEMP/velnor/cargo-clean/fetch-downloads\"";
    let fetch = format!("{base} fetch --locked{manifest} 2> {log}");
    format!(
        "{}if {probe}; then echo \"velnor: sources hit, skipping fetch\"; echo \"velnor: sources downloaded=0\"; else echo \"velnor: sources miss (source_missing), fetching\"; status=0; {fetch} || status=$?; cat {log} >&2; grep -Ec '^ *Downloaded [^ ]+ v[0-9]' {log} > {count} || true; n=; read -r n < {count} || true; echo \"velnor: sources downloaded=${{n:-0}}\"; exit $status; fi",
        cargo_isolation_prefix()
    )
}

fn workspace_manifest(root: &str) -> String {
    if root.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{root}/Cargo.toml")
    }
}
