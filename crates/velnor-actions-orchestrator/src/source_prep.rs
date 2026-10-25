use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::{Step, StepRole};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};

pub(crate) const FETCH_SOURCES_STEP: &str = "Fetch Cargo sources";

pub(crate) const CARGO_CLEAN_DIR: &str = "$RUNNER_TEMP/velnor/cargo-clean";

pub(crate) fn cargo_isolation_prefix() -> String {
    format!("mkdir -p \"{CARGO_CLEAN_DIR}\" && cd \"{CARGO_CLEAN_DIR}\" && ")
}

pub(crate) fn workspace_root_for_manifest<'a>(
    manifest: &str,
    fetch_roots: &'a [String],
) -> &'a str {
    let mut matching = "";
    for root in fetch_roots {
        if !root.is_empty()
            && (manifest == root || manifest.starts_with(&format!("{root}/")))
            && root.len() > matching.len()
        {
            matching = root;
        }
    }
    matching
}

pub(crate) fn crate_fetch_roots(
    manifest: &str,
    fetch_roots: &[String],
    needs_all_workspaces: bool,
) -> Vec<String> {
    if needs_all_workspaces {
        return fetch_roots.to_vec();
    }
    let root = workspace_root_for_manifest(manifest, fetch_roots);
    fetch_roots
        .iter()
        .filter(|candidate| candidate.as_str() == root)
        .cloned()
        .collect()
}

pub(crate) fn isolated_manifest_flag(root: &str) -> String {
    format!(
        "--manifest-path \"$GITHUB_WORKSPACE/{}\"",
        workspace_manifest(root)
    )
}

pub(crate) fn privilege_drop_argv(install: &str, payload: &str) -> Vec<String> {
    let prelude = velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude();
    let script = format!(
        "{{ {install} && {prelude} }} && {}{payload}",
        cargo_isolation_prefix(),
    );
    vec!["sh".to_owned(), "-c".to_owned(), script]
}

const NATIVE_VALIDATORS_SCRIPT: &str = r#"fail=0; if command -v claude >/dev/null 2>&1; then claude plugin validate --strict . || fail=1; fi; if command -v muse >/dev/null 2>&1; then muse plugins validate . || fail=1; for d in skills/*/; do [ -d "$d" ] || continue; muse skills validate "$d" || fail=1; done; fi; if command -v grok >/dev/null 2>&1; then grok plugin validate . || fail=1; fi; exit $fail"#;

pub(crate) fn native_validators_argv() -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        NATIVE_VALIDATORS_SCRIPT.to_owned(),
    ]
}

pub(crate) fn lockful_roots(root: &Path, workspaces: &[PlannedWorkspace]) -> Vec<String> {
    let mut roots = Vec::new();
    for workspace in workspaces {
        let lock = workspace_lock(&workspace.record.workspace_root);
        if root.join(&lock).is_file() {
            roots.push(workspace.record.workspace_root.clone());
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

pub(crate) fn fetch_steps_for_crate(
    catalog: &ToolCatalog,
    roots: &[String],
    all_features: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    fetch_steps_with(catalog, roots, &env, false, all_features)
}

pub(crate) fn fetch_steps_for_plan(
    catalog: &ToolCatalog,
    roots: &[String],
    all_features: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    fetch_steps_with(catalog, roots, &env, false, all_features)
}

pub(crate) fn fetch_steps_for_preview(
    catalog: &ToolCatalog,
    roots: &[String],
    env: &BTreeMap<String, String>,
) -> Result<Vec<Step>, OrchestratorError> {
    fetch_steps_with(catalog, roots, env, true, false)
}

fn fetch_steps_with(
    catalog: &ToolCatalog,
    roots: &[String],
    env: &BTreeMap<String, String>,
    remove_credentials: bool,
    all_features: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(roots.len());
    for root in roots {
        validate_root(root)?;
        let mut script = fetch_script(catalog, root, all_features);
        if remove_credentials {
            let credentials = velnor_actions_workflow_renderer::toolchain_env::credential_scrub()
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
        let mut step = velnor_actions_workflow_renderer::ambient_shell_step(
            &name,
            vec!["sh".to_owned(), "-c".to_owned(), script],
            env.clone(),
        )
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
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

pub(crate) fn validate_root(root: &str) -> Result<(), OrchestratorError> {
    velnor_actions_contract::validate_fetch_root(root).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}
