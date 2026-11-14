// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde_json::Value;
use velnor_local::event::{MergeGroup, NO_COMMIT, PullRequest, Push, Repository, for_standin};
use velnor_local::io::checkout::copy_untracked;
use velnor_local::scenario::{Scenario, Step, next_patch, overlapping};

const SHA_A: &str = "007c9fa8aac37d2aa7bc4a26876bf2654699f6bc";
const SHA_B: &str = "b151fd5994a1e3e2b2ba84bf7d3c3d1e5a5e4f60";

fn read(relative: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    std::fs::read(&path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
}

fn kind(value: Option<&Value>) -> &'static str {
    match value {
        None => "absent",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
        Some(Value::String(text))
            if text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            "sha"
        }
        Some(Value::String(_)) => "string",
    }
}

fn field<'a>(payload: &'a Value, dotted: &str) -> Option<&'a Value> {
    dotted
        .split('.')
        .try_fold(payload, |value, name| value.get(name))
}

fn assert_like_github(leg: &str, payload: &Value) {
    let observed: Value = serde_json::from_slice(&read("conformance/event-payload.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let leg = &observed["legs"][leg];
    let fields = leg["fields"].as_object().into_iter().flatten();
    for (name, expected) in fields {
        assert_eq!(
            kind(field(payload, name)),
            expected.as_str().unwrap_or_default(),
            "{name}"
        );
    }
    let mut top_level: Vec<&str> = payload
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys().map(String::as_str))
        .collect();
    top_level.sort_unstable();
    let expected: Vec<&str> = leg["top_level"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(top_level, expected);
}

const REPOSITORY: Repository<'static> = Repository {
    full_name: "tailrocks/velnor-lab",
    default_branch: "main",
};

#[test]
fn a_push_payload_has_the_shape_github_sent_in_the_probe() {
    let payload = REPOSITORY.push(&Push {
        branch: "main",
        before: NO_COMMIT,
        after: SHA_A,
    });
    assert_like_github("push", &payload);
    assert_eq!(payload["ref"], "refs/heads/main");
    assert_eq!(payload["created"], true);
}

#[test]
fn a_pull_request_payload_has_the_shape_github_sent_in_the_probe() {
    let pull = |action, before, merged, closed_at| PullRequest {
        action,
        number: 3,
        base: ("main", SHA_A),
        head: ("warm", SHA_B),
        before,
        merged,
        merge_commit: SHA_B,
        closed_at,
        changed_files: 1,
    };
    let opened = REPOSITORY.pull_request(&pull("opened", None, false, None));
    assert_like_github("pull_request", &opened);
    assert_eq!(opened["pull_request"]["base"]["sha"], SHA_A);
    assert_eq!(opened["pull_request"]["head"]["ref"], "warm");
    assert_eq!(opened["number"], 3);
    let pushed = REPOSITORY.pull_request(&pull("synchronize", Some(SHA_A), false, None));
    assert_eq!(pushed["action"], "synchronize");
    assert_eq!(
        (&pushed["before"], &pushed["after"]),
        (&SHA_A.into(), &SHA_B.into())
    );
    assert_eq!(opened["pull_request"]["closed_at"], Value::Null);
    assert_eq!(opened["pull_request"]["merge_commit_sha"], SHA_B);
    let at = "2026-11-14T00:00:00Z";
    let merged = REPOSITORY.pull_request(&pull("closed", None, true, Some(at)));
    assert_eq!(merged["pull_request"]["state"], "closed");
    assert_eq!(merged["pull_request"]["merged"], true);
    assert_eq!(merged["pull_request"]["merged_at"], at);
    assert_eq!(merged["pull_request"]["closed_at"], at);
    let closed = REPOSITORY.pull_request(&pull("closed", None, false, Some(at)));
    assert_eq!(closed["pull_request"]["merged_at"], Value::Null);
    assert_eq!(closed["pull_request"]["closed_at"], at);
}

#[test]
fn schedule_and_workflow_dispatch_payloads_have_the_shape_github_sent_in_the_probe() {
    let scheduled = REPOSITORY.schedule("25 6 * * 1", ".github/workflows/ci.yml");
    assert_like_github("schedule", &scheduled);
    assert_eq!(scheduled["schedule"], "25 6 * * 1");
    assert!(for_standin(scheduled).get("workflow").is_none());
    let inputs = serde_json::json!({"tag": "x"});
    let dispatched = REPOSITORY.workflow_dispatch("main", &inputs, ".github/workflows/ci.yml");
    assert_like_github("workflow_dispatch", &dispatched);
    assert_eq!(dispatched["ref"], "refs/heads/main");
    assert_eq!(dispatched["inputs"]["tag"], "x");
}

#[test]
fn a_merge_group_payload_names_the_queue_branch_and_its_base() {
    let queue = "gh-readonly-queue/main/pr-3-007c9fa8aac37d2aa7bc4a26876bf2654699f6bc";
    let payload = REPOSITORY.merge_group(&MergeGroup {
        base: ("main", SHA_A),
        head: (queue, SHA_B),
    });
    assert_eq!(payload["action"], "checks_requested");
    let group = &payload["merge_group"];
    assert_eq!(group["head_ref"], format!("refs/heads/{queue}"));
    assert_eq!(group["head_sha"], SHA_B);
    assert_eq!(group["base_ref"], "refs/heads/main");
    assert_eq!(group["base_sha"], SHA_A);
    assert_eq!(group["head_commit"]["id"], SHA_B);
}

#[test]
fn every_shipped_scenario_parses_and_the_story_runs_in_the_order_the_issue_gives() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios");
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| unreachable!("{e}")) {
        let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
        let scenario = Scenario::parse(&std::fs::read(&path).unwrap_or_default())
            .unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
            .on_default_branch("main");
        assert!(
            scenario.refuse_options(["main"]).is_ok(),
            "{}",
            path.display()
        );
        names.extend(
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned),
        );
    }
    names.sort();
    assert_eq!(
        names,
        ["cold", "docs", "hub", "leaf", "lock", "story", "warm"]
    );
    let story = Scenario::parse(&read("scenarios/story.json"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .on_default_branch("main");
    let shape: Vec<String> = story
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::OpenPullRequest { head, .. } => Some(format!("open {head}")),
            Step::MergePullRequest(head) => Some(format!("merge {head}")),
            Step::ClosePullRequest(head) => Some(format!("close {head}")),
            Step::Push(head) => Some(format!("push {head}")),
            _ => None,
        })
        .collect();
    assert_eq!(
        shape,
        [
            "open first",
            "merge first",
            "open second",
            "push second",
            "open third",
            "close second"
        ]
    );
}

#[test]
fn the_warm_scenario_follows_the_default_branch_and_unknown_steps_option_values_and_escaping_paths_are_refused()
 {
    let warm = Scenario::parse(&read("scenarios/warm.json"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .on_default_branch("trunk");
    assert_eq!(
        warm.steps,
        [
            Step::Push("trunk".to_owned()),
            Step::Commit {
                branch: "warm".to_owned(),
                append: "rust-toolchain.toml".to_owned(),
                line: "# velnor-local warm".to_owned(),
            },
            Step::OpenPullRequest {
                head: "warm".to_owned(),
                base: "trunk".to_owned(),
            },
        ]
    );
    let unknown = br#"{"description": "x", "steps": [{"rebase": "main"}]}"#;
    assert!(Scenario::parse(unknown).is_err());
    assert!(
        warm.refuse_options(["tailrocks/velnor-lab", "main"])
            .is_ok()
    );
    assert!(warm.refuse_options(["--default-branch=x"]).is_err());
    let option = br#"{"description": "x", "steps": [{"push": "--upload-pack=x"}]}"#;
    let option = Scenario::parse(option).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(option.refuse_options(["main"]).is_err());
    let escape = br#"{"description": "x", "steps": [{"commit": {"branch": "b", "append": "../outside", "line": "x"}}]}"#;
    let escape = Scenario::parse(escape).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(escape.refuse_options(["main"]).is_err());
}

#[test]
fn a_state_directory_that_overlaps_the_checkout_is_refused() {
    let checkout = Path::new("/work/lab");
    assert!(overlapping(Path::new("/work/lab/state"), checkout));
    assert!(overlapping(Path::new("/work"), checkout));
    assert!(overlapping(checkout, checkout));
    assert!(!overlapping(Path::new("/work/lab-state"), checkout));
    assert!(!overlapping(Path::new("/tmp/state"), checkout));
}

#[tokio::test]
async fn untracked_files_and_inside_links_are_copied_and_escaping_links_are_refused() {
    let root = crate::support::temp_dir("untracked-links");
    let (checkout, copy, outside) = (
        root.join("checkout"),
        root.join("copy"),
        root.join("outside"),
    );
    for dir in [checkout.join("docs"), copy.clone(), outside.clone()] {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let make = |result: std::io::Result<()>| result.unwrap_or_else(|e| unreachable!("{e}"));
    make(std::fs::write(checkout.join("note.txt"), "note"));
    make(std::fs::write(outside.join("secret.txt"), "secret"));
    make(std::os::unix::fs::symlink(
        "../note.txt",
        checkout.join("docs/latest"),
    ));
    make(std::os::unix::fs::symlink(
        "../outside/secret.txt",
        checkout.join("out"),
    ));
    make(std::os::unix::fs::symlink(
        &outside,
        checkout.join("escape"),
    ));
    make(std::os::unix::fs::symlink(
        "escape/secret.txt",
        checkout.join("alias"),
    ));
    make(std::os::unix::fs::symlink(
        "../../nowhere",
        checkout.join("dangling"),
    ));
    let copied = |path: &'static str| copy_untracked(&checkout, &copy, Path::new(path));
    assert!(copied("note.txt").await.is_ok());
    assert_eq!(
        std::fs::read_to_string(copy.join("note.txt"))
            .ok()
            .as_deref(),
        Some("note")
    );
    assert!(copied("docs/latest").await.is_ok());
    let link = std::fs::read_link(copy.join("docs/latest")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(link, Path::new("../note.txt"));
    for escaping in ["out", "alias", "dangling"] {
        let refused = copied(escaping).await.map_err(|e| e.to_string());
        assert!(
            refused.is_err_and(|message| message.contains(escaping)),
            "{escaping}"
        );
        assert!(
            std::fs::symlink_metadata(copy.join(escaping)).is_err(),
            "{escaping}"
        );
    }
}

#[test]
fn the_lock_step_bumps_the_patch_version_it_finds_in_the_lock_file() {
    let lock = "[[package]]\nname = \"fnv-other\"\nversion = \"2.3.9\"\n\n[[package]]\nname = \"fnv\"\nversion = \"1.0.5\"\nsource = \"registry\"\n";
    let bump = |package| next_patch(lock, package);
    assert_eq!(
        bump("fnv"),
        Some(("fnv@1.0.5".to_owned(), "1.0.6".to_owned()))
    );
    assert_eq!(
        bump("fnv-other"),
        Some(("fnv-other@2.3.9".to_owned(), "2.3.10".to_owned()))
    );
    assert_eq!(bump("serde"), None);
}
