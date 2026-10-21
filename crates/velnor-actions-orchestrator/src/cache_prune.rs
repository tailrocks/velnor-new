//! Per-family pruning of superseded cache entries (cache decision 4).
//!
//! A separate push-only job holds `actions: write` so pull-request runs never
//! get that permission and the Plan job keeps read-only Actions access. For
//! each typed family it lists the `trusted` default-branch entries by key
//! prefix, groups them by key without the final snapshot digest (one cache
//! unit: a root, a workspace, a version) and deletes all but the newest
//! `retained_entries` of each group. Other families, the `pr` trust scope and
//! every other ref are never listed, so nothing outside a family is touched.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::permissions::PermissionLevel;
use velnor_actions_contract::{Job, JobTimeout, Permissions};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::{CACHE_PRUNE_JOB_ID, FINAL_JOB_ID};
use velnor_actions_workflow_renderer::steps::ambient_shell_step;
use velnor_actions_workflow_renderer::toolchain_env::credential_scrub;

use crate::OrchestratorError;

/// Job-level condition: protected default-branch pushes only, after the verdict.
pub(crate) const PRUNE_JOB_CONDITION: &str = "always() && !cancelled() && github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";
/// Step display name.
pub(crate) const PRUNE_STEP_NAME: &str = "Prune superseded cache entries";
const DEFAULT_REF_ENV: &str = "VELNOR_DEFAULT_REF";
const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";

/// Job permissions: the only Actions-write scope in the workflow.
pub(crate) fn prune_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::Write,
        packages: PermissionLevel::None,
    }
}

/// `(key prefix, newest entries kept per unit)` for every family, in family order.
pub(crate) fn prune_targets() -> Vec<(String, u64)> {
    CacheFamily::ALL
        .into_iter()
        .map(|family| {
            (
                format!("{}trusted-", family.key_prefix()),
                family.spec().retained_entries,
            )
        })
        .collect()
}

fn script(gh: &[String]) -> String {
    let mut lines = vec![
        "set -eu".to_owned(),
        // The pinned `gh` through Mise: job images ship no GitHub CLI.
        format!("gh() {{ {} \"$@\"; }}", gh.join(" ")),
        format!("ref=\"${{{DEFAULT_REF_ENV}:?missing default ref}}\""),
        "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac".to_owned(),
        "d=\"$RUNNER_TEMP/velnor/cache-prune\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        "warn() { printf '%s\\n' \"::warning::cache_unavailable: $1\"; }".to_owned(),
        // Newest first; a unit is the key without its final snapshot digest.
        "prune() { if gh cache list --key \"$1\" --ref \"$ref\" --limit 1000 --sort created_at --order desc --json id,key --jq '.[] | [.id, .key] | @tsv' > \"$d/list\"; then :; else warn \"could not list $1\"; return 0; fi; awk -v keep=\"$2\" '{ g = $2; sub(/-s256-[0-9a-f]+$/, \"\", g); n[g]++; if (n[g] > keep) print $1 }' \"$d/list\" > \"$d/doomed\"; while read -r id; do if gh cache delete \"$id\"; then printf 'velnor: pruned superseded entry %s of %s\\n' \"$id\" \"$1\"; else warn \"could not delete $id\"; fi; done < \"$d/doomed\"; }".to_owned(),
    ];
    for (prefix, keep) in prune_targets() {
        lines.push(format!("prune '{prefix}' {keep}"));
    }
    lines.join("; ")
}

/// The prune job, or `None` when the workflow writes no cache family.
///
/// # Errors
///
/// Returns render errors for a rejected step shape.
pub(crate) fn prune_job(
    label: &str,
    catalog: &ToolCatalog,
    extra_needs: &[String],
) -> Result<(String, Job), OrchestratorError> {
    let gh = crate::vectors::exec_argv(vec![PinnedTool::Gh], "gh", &[], catalog)?;
    let mut env: BTreeMap<String, String> = credential_scrub();
    env.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
    env.insert(DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned());
    let step = ambient_shell_step(
        PRUNE_STEP_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script(&gh)],
        env,
    )?;
    let mut needs = vec![FINAL_JOB_ID.to_owned()];
    needs.extend(extra_needs.iter().cloned());
    Ok((
        CACHE_PRUNE_JOB_ID.to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Cache prune".to_owned(),
            runs_on: label.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs,
            condition: Some(PRUNE_JOB_CONDITION.to_owned()),
            permissions: Some(prune_permissions()),
            environment: None,
            steps: vec![
                super::prepare_pinned_tools_step(catalog, vec![PinnedTool::Gh], true)?,
                step,
            ],
            cache_mode: None,
        },
    ))
}

#[cfg(test)]
#[path = "cache_prune_tests.rs"]
mod tests;
