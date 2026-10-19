use std::collections::BTreeMap;

use velnor_actions_contract::StepKind;

use super::*;

fn script(fetch_roots: &[String], tofu_roots: &[String]) -> (String, BTreeMap<String, String>) {
    let step = cache_keys_step(&ToolCatalog::pinned(), fetch_roots, tofu_roots)
        .expect("builds")
        .expect("emitted");
    assert_eq!(step.id, Some(StepId::CacheKeys));
    assert_eq!(step.role, Some(StepRole::CacheKeys));
    let StepKind::Shell { run, env } = step.kind else {
        panic!("shell step");
    };
    (run[2].clone(), env)
}

/// Run the generated script under `event` and return its parsed outputs.
fn run(event: &[(&str, &str)], label: &str) -> BTreeMap<String, String> {
    let dir =
        std::env::temp_dir().join(format!("velnor-cache-keys-{label}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let workspace = dir.join("ws");
    std::fs::create_dir_all(workspace.join("nested")).expect("workspace");
    std::fs::create_dir_all(dir.join("tmp")).expect("runner temp");
    std::fs::write(workspace.join("Cargo.lock"), "root-lock").expect("lock");
    std::fs::write(workspace.join("nested/Cargo.lock"), "nested-lock").expect("lock");
    let output = dir.join("github-output");
    std::fs::write(&output, "").expect("output");
    let (script, _) = script(&[String::new(), "nested".to_owned()], &[]);
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&script)
        .env("RUNNER_TEMP", dir.join("tmp"))
        .env("GITHUB_WORKSPACE", &workspace)
        .env("GITHUB_OUTPUT", &output)
        .envs(event.iter().copied())
        .status()
        .expect("sh");
    assert!(status.success(), "{label}");
    let text = std::fs::read_to_string(&output).expect("outputs");
    let mut outputs = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some((name, delimiter)) = line.split_once("<<") {
            let mut value = Vec::new();
            for next in lines.by_ref() {
                if next == delimiter {
                    break;
                }
                value.push(next);
            }
            outputs.insert(name.to_owned(), value.join("\n"));
        } else if let Some((name, value)) = line.split_once('=') {
            outputs.insert(name.to_owned(), value.to_owned());
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    outputs
}

const MAIN: &str = "refs/heads/main";

#[test]
fn trusted_push_keys_the_trusted_namespace_and_may_save() {
    let out = run(
        &[
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_REF", MAIN),
            ("GITHUB_REF_PROTECTED", "true"),
            (DEFAULT_REF_ENV, MAIN),
        ],
        "trusted",
    );
    assert_eq!(out["trust"], "trusted");
    assert_eq!(out["sources_save"], "true");
    let key = &out["sources_key"];
    assert!(key.starts_with("velnor-v2-sources-trusted-s256-"), "{key}");
    assert_eq!(key.len(), CacheFamily::Sources.max_key_bytes());
    let prefix = &key[..key.len() - 69];
    assert_eq!(out["sources_restore_keys"], prefix);
    assert!(key.ends_with(&out["sources_snapshot"]));
}

#[test]
fn same_repository_pr_writes_pr_scope_and_never_restores_a_pr_prefix() {
    let out = run(
        &[
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_REF", "refs/pull/7/merge"),
            (HEAD_FORK_ENV, "false"),
            (DEFAULT_REF_ENV, MAIN),
        ],
        "pr",
    );
    assert_eq!(out["trust"], "pr");
    assert_eq!(out["sources_save"], "true");
    let key = &out["sources_key"];
    assert!(key.starts_with("velnor-v2-sources-pr-s256-"), "{key}");
    let restore: Vec<&str> = out["sources_restore_keys"].lines().collect();
    assert_eq!(restore.len(), 2);
    assert!(restore[0].starts_with("velnor-v2-sources-trusted-"));
    assert!(restore[0].ends_with(&out["sources_snapshot"]));
    assert!(restore[1].ends_with('-') && restore[0].starts_with(restore[1]));
    assert!(restore.iter().all(|key| !key.contains("-pr-")));
}

#[test]
fn forks_merge_groups_and_unprotected_pushes_never_write() {
    for (label, event) in [
        (
            "fork",
            vec![
                ("GITHUB_EVENT_NAME", "pull_request"),
                (HEAD_FORK_ENV, "true"),
                (DEFAULT_REF_ENV, MAIN),
            ],
        ),
        (
            "merge-group",
            vec![
                ("GITHUB_EVENT_NAME", "merge_group"),
                (DEFAULT_REF_ENV, MAIN),
            ],
        ),
        (
            "unprotected",
            vec![
                ("GITHUB_EVENT_NAME", "push"),
                ("GITHUB_REF", MAIN),
                ("GITHUB_REF_PROTECTED", "false"),
                (DEFAULT_REF_ENV, MAIN),
            ],
        ),
        (
            "branch",
            vec![
                ("GITHUB_EVENT_NAME", "push"),
                ("GITHUB_REF", "refs/heads/topic"),
                ("GITHUB_REF_PROTECTED", "true"),
                (DEFAULT_REF_ENV, MAIN),
            ],
        ),
    ] {
        let out = run(&event, label);
        assert_eq!(out["trust"], "none", "{label}");
        assert_eq!(out["sources_save"], "false", "{label}");
        assert!(
            out["sources_key"].starts_with("velnor-v2-sources-trusted-"),
            "{label}: none reads the trusted namespace"
        );
    }
}

#[test]
fn compat_excludes_the_toolchain_and_snapshot_tracks_lockfiles() {
    let (script, env) = script(&[String::new()], &[]);
    assert!(!script.contains("rust@"), "no toolchain in sources compat");
    assert!(!script.contains("hashFiles"));
    assert!(script.contains("$RUNNER_TEMP/velnor/cargo/registry/index"));
    assert!(script.contains("s256 \"$GITHUB_WORKSPACE/Cargo.lock\""));
    assert_eq!(
        env.get(HEAD_FORK_ENV).map(String::as_str),
        Some(HEAD_FORK_EXPR)
    );
    assert!(
        cache_keys_step(&ToolCatalog::pinned(), &[], &[])
            .expect("builds")
            .is_none()
    );
}

#[test]
fn tofu_roots_get_ordinal_keys_bound_to_tofu_version() {
    let (script, _) = script(&[], &["stacks/a".to_owned(), "stacks/b".to_owned()]);
    for output in ["tofu_providers_key_0=", "tofu_providers_key_1="] {
        assert!(script.contains(output), "{output}");
    }
    assert!(script.contains("-- tofu version"));
    assert!(script.contains("$GITHUB_WORKSPACE/stacks/b/.terraform.lock.hcl"));
    assert!(script.contains("velnor-v2-tofu-providers-trusted-"));
    assert!(!script.contains("sources_key="));
}
