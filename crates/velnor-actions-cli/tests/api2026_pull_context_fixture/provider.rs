// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path};

use super::{Identities, Paths, REPOSITORY_ID, Scenario};

pub(crate) fn write(
    paths: &Paths,
    ids: &Identities,
    scenario: Scenario,
) -> Result<(), Box<dyn Error>> {
    let bin = paths.root.join("bin");
    fs::create_dir_all(&bin)?;
    let state = paths.root.join("provider");
    fs::create_dir_all(&state)?;
    let failure = match scenario {
        Scenario::HttpFailure => "http",
        Scenario::StdoutLimit => "stdout",
        Scenario::StderrLimit => "stderr",
        Scenario::Deadline => "deadline",
        Scenario::MergePendingDeadline => "merge-deadline",
        Scenario::MergePendingApiDeadline => "merge-api-deadline",
        _ => "none",
    };
    fs::write(state.join("failure"), failure)?;
    let mergeability = match scenario {
        Scenario::MergePendingReady => "ready",
        Scenario::MergePendingExhausted
        | Scenario::MergePendingDeadline
        | Scenario::MergePendingApiDeadline => "pending",
        Scenario::MergePendingFalse => "false",
        Scenario::MergePendingChanged => "changed",
        _ => "none",
    };
    fs::write(state.join("mergeability"), mergeability)?;
    write_identity(&state, ids, mergeability)?;
    write_merge(&state, ids, scenario)?;
    write_lineage(&state, ids)?;
    write_source_trees(&state, ids)?;
    write_programs(&bin, &state)
}

fn write_identity(
    state: &Path,
    ids: &Identities,
    mergeability: &str,
) -> Result<(), Box<dyn Error>> {
    response(
        state,
        "repository.json",
        &serde_json::json!({
            "id":REPOSITORY_ID,"full_name":"test-owner/test-tap","default_branch":"main"
        }),
    )?;
    let mut pull = serde_json::json!({
        "number":17,"state":"open","draft":false,"merged_at":null,"mergeable":true,
        "base":{"sha":ids.base,"ref":"main","repo":{"id":REPOSITORY_ID,"full_name":"test-owner/test-tap"}},
        "head":{"sha":ids.head,"ref":"fixture-head","repo":{"id":REPOSITORY_ID,"full_name":"test-owner/test-tap"}}
    });
    if mergeability != "none" {
        pull["mergeable"] = serde_json::Value::Null;
    }
    response(state, "pull.json", &pull)?;
    pull["mergeable"] = serde_json::json!(mergeability != "false");
    if mergeability == "changed" {
        pull["head"]["sha"] = serde_json::json!("9999999999999999999999999999999999999999");
    }
    response(state, "pull-ready.json", &pull)
}

fn write_merge(state: &Path, ids: &Identities, scenario: Scenario) -> Result<(), Box<dyn Error>> {
    let parents = match scenario {
        Scenario::ReversedParents => vec![
            serde_json::json!({"sha":ids.head}),
            serde_json::json!({"sha":ids.base}),
        ],
        _ => vec![
            serde_json::json!({"sha":ids.base}),
            serde_json::json!({"sha":ids.head}),
        ],
    };
    response(
        state,
        "merge-commit.json",
        &serde_json::json!({"sha":ids.merge,"parents":parents}),
    )?;
    let changed = "9999999999999999999999999999999999999999";
    response(state, "merge-ref-first.json", &refs(&ids.merge))?;
    response(
        state,
        "merge-ref-second.json",
        &refs(match scenario {
            Scenario::ChangedRef => changed,
            _ => &ids.merge,
        }),
    )?;
    Ok(())
}

fn write_lineage(state: &Path, ids: &Identities) -> Result<(), Box<dyn Error>> {
    response(
        state,
        "head-commit.json",
        &serde_json::json!({"sha":ids.head,"parents":[{"sha":ids.base}]}),
    )?;
    response(
        state,
        "main-ref.json",
        &serde_json::json!([{"ref":"refs/heads/main","object":{"sha":ids.base,"type":"commit"}}]),
    )?;
    response(
        state,
        "branch.json",
        &serde_json::json!({
            "name":"main","protected":true,"commit":{"sha":ids.base}
        }),
    )?;
    response(
        state,
        "compare.json",
        &serde_json::json!({"base_commit":{"sha":ids.base},"merge_base_commit":{"sha":ids.base},"status":"ahead","ahead_by":1,"total_commits":1}),
    )?;
    Ok(())
}

fn write_source_trees(state: &Path, ids: &Identities) -> Result<(), Box<dyn Error>> {
    response(
        state,
        "base-commit.json",
        &serde_json::json!({"sha":ids.base,"tree":{"sha":ids.root_tree}}),
    )?;
    response(
        state,
        "root-tree.json",
        &tree(&ids.root_tree, &[dir(".velnor", &ids.vel_nor_tree)]),
    )?;
    response(
        state,
        "velnor-tree.json",
        &tree(
            &ids.vel_nor_tree,
            &[blob("config.toml", "100644", &ids.head)],
        ),
    )?;
    Ok(())
}

fn write_programs(bin: &Path, state: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(state.join("ref-count"), "0")?;
    fs::write(state.join("pull-count"), "0")?;
    fs::write(state.join("requests.log"), "")?;
    fs::write(
        bin.join("mise"),
        MISE.replace(
            "@VELNOR_GH_SELECTOR@",
            &format!("gh@{}", velnor_actions_mise::catalog::GH_VERSION),
        ),
    )?;
    fs::write(bin.join("gh"), GH)?;
    executable(&bin.join("mise"))?;
    executable(&bin.join("gh"))?;
    Ok(())
}

fn refs(sha: &str) -> serde_json::Value {
    serde_json::json!([{"ref":"refs/pull/17/merge","object":{"sha":sha,"type":"commit"}}])
}

fn dir(path: &str, sha: &str) -> serde_json::Value {
    serde_json::json!({"path":path,"mode":"040000","type":"tree","sha":sha})
}

fn blob(path: &str, mode: &str, sha: &str) -> serde_json::Value {
    serde_json::json!({"path":path,"mode":mode,"type":"blob","sha":sha})
}

fn tree(sha: &str, entries: &[serde_json::Value]) -> serde_json::Value {
    serde_json::json!({"sha":sha,"truncated":false,"tree":entries})
}

fn response(state: &Path, name: &str, value: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    fs::write(state.join(name), value.to_string())?;
    Ok(())
}

fn executable(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

const MISE: &str = r#"#!/bin/sh
printf 'mise argc=%s prefix=%s,%s,%s,%s,%s,%s,%s\n' "$#" "${1-}" "${2-}" "${3-}" "${4-}" "${5-}" "${6-}" "${7-}" >> "$VELNOR_FIXTURE_STATE/requests.log"
[ "$1" = "--no-config" ] && [ "$2" = "--no-env" ] && [ "$3" = "--no-hooks" ] && [ "$4" = "exec" ] || exit 91
[ "$5" = "@VELNOR_GH_SELECTOR@" ] && [ "$6" = "--" ] && [ "$7" = "gh" ] || exit 92
shift 6
exec "$@"
"#;

const GH: &str = r#"#!/bin/sh
printf 'gh argc=%s prefix=%s,%s,%s,%s,%s\n' "$#" "${1-}" "${2-}" "${3-}" "${4-}" "${5-}" >> "$VELNOR_FIXTURE_STATE/requests.log"
[ "$#" -eq 10 ] || { printf 'gh argument count rejected: %s\n' "$#" >> "$VELNOR_FIXTURE_STATE/requests.log"; exit 82; }
[ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$3" = "Accept: application/vnd.github+json" ] || exit 81
[ "$4" = "-H" ] && [ "$5" = "X-GitHub-Api-Version: 2026-03-10" ] || exit 85
route=$6
[ "$7" = "-H" ] && [ "$8" = "Cache-Control: no-cache, no-store" ] || exit 86
[ "$9" = "-H" ] && [ "${10}" = "Pragma: no-cache" ] || exit 87
strip_read_nonce() {
  case "$route" in
    *\&_velnor_read_nonce=*)
      nonce=${route##*&_velnor_read_nonce=}
      case "$nonce" in ''|*[!0123456789abcdef-]*) return 1 ;; esac
      route=${route%&_velnor_read_nonce=$nonce}
      ;;
    *\?_velnor_read_nonce=*)
      nonce=${route##*\?_velnor_read_nonce=}
      case "$nonce" in ''|*[!0123456789abcdef-]*) return 1 ;; esac
      route=${route%\?_velnor_read_nonce=$nonce}
      ;;
  esac
}
strip_read_nonce || exit 88
printf 'route=%s\n' "$route" >> "$VELNOR_FIXTURE_STATE/requests.log"
if [ "$route" = repos/test-owner/test-tap ]; then
  case "$(cat "$VELNOR_FIXTURE_STATE/failure")" in
    http)
      printf 'gh: diagnostic fixture (HTTP 403)\nfixture-secret-canary https://redirect.invalid/private?credential=fixture-signed-canary\033[31m\n' >&2
      exit 1
      ;;
    stdout) dd if=/dev/zero bs=1048577 count=1 2>/dev/null; exit 0 ;;
    stderr) dd if=/dev/zero bs=1048577 count=1 1>&2 2>/dev/null; exit 1 ;;
    deadline) exec /bin/sleep 15 ;;
  esac
fi
case "$route" in
  repos/test-owner/test-tap) file=repository.json ;;
  repos/test-owner/test-tap/pulls/17)
    count=$(cat "$VELNOR_FIXTURE_STATE/pull-count")
    count=$((count + 1))
    printf '%s' "$count" > "$VELNOR_FIXTURE_STATE/pull-count"
    if [ "$(cat "$VELNOR_FIXTURE_STATE/failure")" = merge-api-deadline ] && [ "$count" -ge 2 ]; then exec /bin/sleep 15; fi
    mode=$(cat "$VELNOR_FIXTURE_STATE/mergeability")
    file=pull.json
    if [ "$mode" != pending ] && [ "$mode" != none ] && [ "$count" -ge 3 ]; then file=pull-ready.json; fi
    ;;
  repos/test-owner/test-tap/git/matching-refs/pull/17/merge)
    mode=$(cat "$VELNOR_FIXTURE_STATE/mergeability")
    pull_count=$(cat "$VELNOR_FIXTURE_STATE/pull-count")
    if [ "$mode" = false ] || { [ "$mode" != none ] && [ "$pull_count" -lt 3 ]; }; then printf '[]'; exit 0; fi
    count=$(cat "$VELNOR_FIXTURE_STATE/ref-count")
    count=$((count + 1))
    printf '%s' "$count" > "$VELNOR_FIXTURE_STATE/ref-count"
    if [ "$count" -eq 1 ]; then file=merge-ref-first.json; else file=merge-ref-second.json; fi
    ;;
  repos/test-owner/test-tap/git/commits/$VELNOR_FIXTURE_MERGE) file=merge-commit.json ;;
  repos/test-owner/test-tap/git/commits/$VELNOR_FIXTURE_BASE) file=base-commit.json ;;
  repos/test-owner/test-tap/git/commits/$VELNOR_FIXTURE_HEAD) file=head-commit.json ;;
  repos/test-owner/test-tap/commits/$VELNOR_FIXTURE_HEAD) file=head-commit.json ;;
  repos/test-owner/test-tap/commits/$VELNOR_FIXTURE_BASE) file=base-commit.json ;;
  repos/test-owner/test-tap/commits/$VELNOR_FIXTURE_MERGE) file=merge-commit.json ;;
  repos/test-owner/test-tap/git/matching-refs/heads/main) file=main-ref.json ;;
  repos/test-owner/test-tap/branches/main) file=branch.json ;;
  repos/test-owner/test-tap/compare/$VELNOR_FIXTURE_BASE...$VELNOR_FIXTURE_HEAD\?per_page=1\&page=2) file=compare.json ;;
  repos/test-owner/test-tap/git/trees/$VELNOR_FIXTURE_ROOT_TREE) file=root-tree.json ;;
  repos/test-owner/test-tap/git/trees/$VELNOR_FIXTURE_VELNOR_TREE) file=velnor-tree.json ;;
  *) printf 'unmapped route=%s\n' "$route" >> "$VELNOR_FIXTURE_STATE/requests.log"; exit 84 ;;
esac
cat "$VELNOR_FIXTURE_STATE/$file"
"#;
