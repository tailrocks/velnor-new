// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::RunsOn;
use velnor_actions_contract::workflow::cache_family::CacheFamily;

use crate::RenderError;
use crate::marker::with_marker;
use crate::render::RenderedFile;

pub const CACHE_PRUNE_PATH: &str = ".github/workflows/cache-prune.yml";
pub const CACHE_PRUNE_FALLBACK_RUNNER: &str = "ubuntu-latest";
pub const TOOLS_PRUNE_PREFIX: &str = "mise-tools-v2-";
pub const TOOLS_PRUNE_UNIT: &str = "mise-tools-v2";
pub const TOOLS_RETAINED_ENTRIES: u64 = 8;

fn strip_snapshot_digest(cache_key: &str) -> Option<&str> {
    let (unit, digest) = cache_key.rsplit_once("-s256-")?;
    (!digest.is_empty()
        && digest
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
    .then_some(unit)
}

#[must_use]
pub fn prune_group_key(cache_key: &str) -> String {
    if cache_key.starts_with(TOOLS_PRUNE_PREFIX) {
        return TOOLS_PRUNE_UNIT.to_owned();
    }
    if let Some(unit) = strip_snapshot_digest(cache_key) {
        return unit.to_owned();
    }
    cache_key.to_owned()
}

#[must_use]
pub fn prune_targets() -> Vec<(String, u64)> {
    let mut targets: Vec<(String, u64)> = CacheFamily::ALL
        .into_iter()
        .map(|family| {
            (
                format!("{}trusted-", family.key_prefix()),
                family.spec().retained_entries,
            )
        })
        .collect();
    targets.push((TOOLS_PRUNE_PREFIX.to_owned(), TOOLS_RETAINED_ENTRIES));
    targets
}

#[must_use]
pub fn prune_script() -> String {
    let mut lines = vec![
        "set -eu".to_owned(),
        "ref=\"$GITHUB_REF\"".to_owned(),
        "d=\"$RUNNER_TEMP/velnor-cache-prune\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        "warn() { printf '%s\\n' \"::warning::cache_unavailable: $1\"; }".to_owned(),
        {
            let group_program = format!(
                "{{ if ($2 ~ /^{TOOLS_PRUNE_PREFIX}/) g = \"{TOOLS_PRUNE_UNIT}\"; else {{ g = $2; sub(/-s256-[0-9a-f]+$/, \"\", g) }}; n[g]++; if (n[g] > keep) print $1 }}",
            );
            format!(
                "prune() {{ if gh cache list --key \"$1\" --ref \"$ref\" --limit 1000 --sort created_at --order desc --json id,key --jq '.[] | [.id, .key] | @tsv' > \"$d/list\"; then :; else warn \"could not list $1\"; return 0; fi; awk -v keep=\"$2\" '{group_program}' \"$d/list\" > \"$d/doomed\"; while read -r id; do if gh cache delete \"$id\"; then printf 'velnor: pruned superseded entry %s of %s\\n' \"$id\" \"$1\"; else warn \"could not delete $id\"; fi; done < \"$d/doomed\"; }}"
            )
        },
    ];
    for (prefix, keep) in prune_targets() {
        lines.push(format!("prune '{prefix}' {keep}"));
    }
    lines.join("; ")
}

pub fn render_cache_prune_yml(
    runs_on: &str,
    default_branch: &str,
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
    let script = prune_script().replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!(
        "name: Cache prune\n\"on\":\n  push:\n    branches:\n      - {default_branch}\npermissions: {{}}\nconcurrency:\n  group: velnor-cache-prune\n  cancel-in-progress: false\njobs:\n  prune:\n    name: Prune superseded cache entries\n    if: github.ref_protected == true\n    runs-on: {label}\n    timeout-minutes: 5\n    permissions:\n      actions: write\n    steps:\n      - name: Prune superseded cache entries\n        env:\n          GH_TOKEN: ${{{{ github.token }}}}\n          GH_REPO: ${{{{ github.repository }}}}\n        run: \"{script}\"\n"
    );
    Ok(RenderedFile {
        path: CACHE_PRUNE_PATH.to_owned(),
        bytes: with_marker(version, &body)?,
    })
}
