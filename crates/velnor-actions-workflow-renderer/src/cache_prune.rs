// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::RunsOn;
use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_PREDICATE_TYPE, STATE_SEAL_WORKFLOW_NAME, state_seal_signer_workflow,
};

use crate::RenderError;
use crate::marker::with_marker;
use crate::render::RenderedFile;
use crate::yaml::quote_scalar;

pub const CACHE_PRUNE_PATH: &str = ".github/workflows/cache-prune.yml";
pub const CACHE_PRUNE_FALLBACK_RUNNER: &str = "ubuntu-latest";
pub const TOOLS_PRUNE_PREFIX: &str = "mise-tools-v2-";
pub const TOOLS_PRUNE_UNIT: &str = "mise-tools-v2";
pub const TOOLS_RETAINED_ENTRIES: u64 = 8;
pub const STATE_PRUNE_VERIFY_CAP: usize = 5;
pub const RETIRED_STATE_PRUNE_PREFIXES: [&str; 3] = [
    "velnor-v1-state-trusted-",
    "velnor-v2-state-trusted-",
    "velnor-v3-state-trusted-",
];

#[must_use]
pub fn prune_targets() -> Vec<(String, u64)> {
    let mut targets: Vec<(String, u64)> = CacheFamily::ALL
        .into_iter()
        .filter(|family| *family != CacheFamily::MbxBundle)
        .map(|family| {
            (
                format!("{}trusted-", family.key_prefix()),
                family.spec().retained_entries,
            )
        })
        .collect();
    targets.push((TOOLS_PRUNE_PREFIX.to_owned(), TOOLS_RETAINED_ENTRIES));
    targets.extend(
        RETIRED_STATE_PRUNE_PREFIXES
            .into_iter()
            .map(|prefix| (prefix.to_owned(), 0)),
    );
    targets
}

#[must_use]
pub fn state_prune_prefix() -> String {
    format!("{}trusted-", CacheFamily::MbxBundle.key_prefix())
}

fn state_group_program() -> String {
    let prefix = state_prune_prefix();
    format!(
        "{{ n = split($2, p, \"-s256-\"); if (n != 4 || p[1] !~ /^{prefix}(union|suite)-[a-z0-9][a-z0-9-]*$/) next; for (i = 2; i <= 4; i++) if (length(p[i]) != 64 || p[i] !~ /^[0-9a-f]+$/) next; print p[1], $1, $2, $3 }}"
    )
}

#[must_use]
pub fn prune_script() -> String {
    let group_program = format!(
        "{{ if ($2 ~ /^{TOOLS_PRUNE_PREFIX}/) g = \"{TOOLS_PRUNE_UNIT}\"; else {{ g = $2; sub(/-s256-[0-9a-f]+$/, \"\", g) }}; n[g]++; if (n[g] > keep) print $1 }}",
    );
    let signer = state_seal_signer_workflow();
    let mut lines = vec![
        "set -eu".to_owned(),
        "ref=\"$GITHUB_REF\"".to_owned(),
        "d=\"$RUNNER_TEMP/velnor-cache-prune\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        "warn() { printf '%s\\n' \"::warning::cache_unavailable: $1\"; }".to_owned(),
        "fetch() { if gh cache list --key \"$1\" --ref \"$ref\" --limit 1000 --sort created_at --order desc --json id,key --jq '.[] | [.id, .key] | @tsv' > \"$d/list\"; then return 0; else warn \"could not list $1\"; return 1; fi; }".to_owned(),
        "sweep() { while read -r id; do if gh cache delete \"$id\"; then printf 'velnor: pruned superseded entry %s of %s\\n' \"$id\" \"$1\"; else warn \"could not delete $id\"; fi; done < \"$d/doomed\"; }".to_owned(),
        format!(
            "prune() {{ if fetch \"$1\"; then awk -v keep=\"$2\" '{group_program}' \"$d/list\" > \"$d/doomed\"; sweep \"$1\"; fi; }}"
        ),
        "fetch_state() { if gh cache list --key \"$1\" --ref \"$ref\" --limit 1000 --sort created_at --order desc --json id,key,sizeInBytes --jq '.[] | [.id, .key, .sizeInBytes] | @tsv' > \"$d/list\"; then return 0; else warn \"could not list $1\"; return 1; fi; }".to_owned(),
        format!(
            "sweep_state() {{ cur=; found=; tried=; while read -r g id key size; do if [ \"$g\" != \"$cur\" ]; then cur=$g; found=; tried=; fi; if [ -n \"$found\" ]; then if gh cache delete \"$id\" < /dev/null; then printf 'velnor: pruned superseded entry %s of %s\\n' \"$id\" \"$1\"; else warn \"could not delete $id\"; fi; elif [ \"${{#tried}}\" -lt {STATE_PRUNE_VERIFY_CAP} ]; then tried=\"${{tried}}x\"; printf '%s %s %s\\n' \"$key\" \"$id\" \"$size\" > \"$d/subject\"; if gh attestation verify \"$d/subject\" -R \"$GITHUB_REPOSITORY\" --signer-workflow \"{signer}\" --source-ref \"$ref\" --predicate-type '{STATE_SEAL_PREDICATE_TYPE}' > /dev/null 2>&1 < /dev/null; then found=x; fi; fi; done < \"$d/groups\"; }}"
        ),
        format!(
            "prune_state() {{ if fetch_state \"$1\"; then awk -F '\\t' '{}' \"$d/list\" > \"$d/rows\"; LC_ALL=C sort -s -k1,1 \"$d/rows\" > \"$d/groups\"; sweep_state \"$1\"; fi; }}",
            state_group_program()
        ),
    ];
    for (prefix, keep) in prune_targets() {
        lines.push(format!("prune '{prefix}' {keep}"));
    }
    lines.push(format!("prune_state '{}'", state_prune_prefix()));
    lines.join("; ")
}

pub fn render_cache_prune_yml(
    runs_on: &str,
    default_branch: &str,
    workflow_name: &str,
    version: &str,
) -> Result<RenderedFile, RenderError> {
    let label = match RunsOn::parse(runs_on).map_err(RenderError::Contract)? {
        RunsOn::Hosted(label) => label,
        RunsOn::ScaleSet(_) => CACHE_PRUNE_FALLBACK_RUNNER.to_owned(),
    };
    let safe = !default_branch.is_empty()
        && default_branch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'));
    if !safe {
        return Err(RenderError::InvalidWorkflow(
            "cache_prune_default_branch".to_owned(),
        ));
    }
    let workflow = quote_scalar(workflow_name);
    let sealed = quote_scalar(STATE_SEAL_WORKFLOW_NAME);
    let script = prune_script().replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!(
        "name: Cache prune\n\"on\":\n  workflow_run:\n    workflows:\n      - {workflow} # zizmor: ignore[dangerous-triggers]\n      - {sealed} # zizmor: ignore[dangerous-triggers]\n    types:\n      - completed\n    branches:\n      - {default_branch}\npermissions: {{}}\ncache-mode: read\nconcurrency:\n  group: velnor-cache-prune\n  cancel-in-progress: false\njobs:\n  prune:\n    name: Prune superseded cache entries\n    if: github.event.workflow_run.event == 'push' && github.event.workflow_run.head_branch == '{default_branch}' && github.event.workflow_run.head_repository.full_name == github.repository && github.event.workflow_run.conclusion != 'cancelled'\n    runs-on: {label}\n    timeout-minutes: 5\n    permissions:\n      actions: write\n      attestations: read\n    steps:\n      - name: Prune superseded cache entries\n        env:\n          GH_TOKEN: ${{{{ github.token }}}}\n          GH_REPO: ${{{{ github.repository }}}}\n        run: \"{script}\"\n"
    );
    Ok(RenderedFile {
        path: CACHE_PRUNE_PATH.to_owned(),
        bytes: with_marker(version, &body)?,
    })
}
