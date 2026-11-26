// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;

use crate::io::Repo;

fn github_env(name: &str, event: &str, git_ref: &str) -> BTreeMap<String, String> {
    let repo = Repo::empty(name);
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(root.join("shared")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(root.join("temp")).unwrap_or_else(|e| unreachable!("{e}"));
    let env_file = root.join("github-env");
    let event_file = root.join("event.json");
    fs::write(&event_file, r#"{"repository":{"default_branch":"main"}}"#)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("mbx-store")
        .env_clear()
        .env("RUNNER_TEMP", root.join("temp"))
        .env("GITHUB_ENV", &env_file)
        .env("GITHUB_EVENT_PATH", &event_file)
        .env("GITHUB_EVENT_NAME", event)
        .env("GITHUB_REF", git_ref)
        .env("GITHUB_REF_PROTECTED", "true")
        .env("VELNOR_MBX_SHARED_DIR", root.join("shared"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{name}: {output:?}");
    fs::read_to_string(&env_file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| {
            let value = value.replace(root.to_str().unwrap_or_default(), "<host>");
            (key.to_owned(), value)
        })
        .collect()
}

#[test]
fn pr_and_push_share_build_env() {
    let push = github_env("mbx-store-parity-push", "push", "refs/heads/main");
    for (name, event, git_ref) in [
        ("mbx-store-parity-pr", "pull_request", "refs/pull/1/merge"),
        (
            "mbx-store-parity-queue",
            "merge_group",
            "refs/heads/gh-readonly-queue/main/pr-1-a",
        ),
    ] {
        let other = github_env(name, event, git_ref);
        // Cargo and build scripts read these: `HOST_CC` and `CC_*` name the shims.
        for variable in ["MBX_SHIMS_DIR", "MBX_SHARE_OUT_DIR"] {
            assert!(push.contains_key(variable), "push sets {variable}");
            assert_eq!(
                push.get(variable),
                other.get(variable),
                "{event} {variable}"
            );
        }
        assert!(push.contains_key("MBX_CACHE_DIR"));
        assert!(
            !other.contains_key("MBX_CACHE_DIR"),
            "{event} stays untrusted"
        );
    }
}
