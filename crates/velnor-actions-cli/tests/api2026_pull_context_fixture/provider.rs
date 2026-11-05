// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path};

use super::{Identities, Paths, REPOSITORY_ID, Scenario};

pub(crate) fn write(
    paths: &Paths,
    ids: &Identities,
    scenario: Scenario,
) -> Result<(), Box<dyn Error>> {
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
    fs::write(state.join("ref-count"), "0")?;
    fs::write(state.join("pull-count"), "0")?;
    fs::write(state.join("requests.log"), "")?;
    Ok(())
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
