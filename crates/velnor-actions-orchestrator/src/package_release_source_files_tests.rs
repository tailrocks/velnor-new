//! Exercise immutable-tree custody without executing repository content.

use super::*;
use serde_json::{Value, json};

fn route(suffix: &str) -> String {
    format!("repos/example/app/{suffix}")
}
fn prefix(entry: &Value) -> Vec<(String, Value)> {
    vec![
        (
            route(&format!("git/commits/{}", "a".repeat(40))),
            json!({"sha":"a".repeat(40),"tree":{"sha":"b".repeat(40)}}),
        ),
        (
            route(&format!("git/trees/{}", "b".repeat(40))),
            json!({"sha":"b".repeat(40),"truncated":false,"tree":[entry]}),
        ),
    ]
}
fn entry(mode: &str, kind: &str) -> Value {
    json!({"path":"Cargo.toml","mode":mode,"type":kind,"sha":"c".repeat(40)})
}
fn recursive_tree(entries: &[Value]) -> Vec<(String, Value)> {
    vec![
        (
            route(&format!("git/commits/{}", "a".repeat(40))),
            json!({"sha":"a".repeat(40),"tree":{"sha":"b".repeat(40)}}),
        ),
        (
            route(&format!("git/trees/{}?recursive=1", "b".repeat(40))),
            json!({"sha":"b".repeat(40),"truncated":false,"tree":entries}),
        ),
    ]
}
fn large_tree_entries(count: usize) -> Vec<Value> {
    (0..count)
        .map(|index| {
            json!({
                "path": large_tree_path(index),
                "mode":"100644",
                "type":"blob",
                "sha":"c".repeat(40)
            })
        })
        .collect()
}
fn large_tree_path(index: usize) -> String {
    let segment = |letter: char| letter.to_string().repeat(220);
    format!(
        "{}-{index}/{}/{}/{}/file",
        segment('a'),
        segment('b'),
        segment('c'),
        segment('d')
    )
}

#[test]
fn exact_commit_tree_and_regular_blob_bytes_are_required() {
    let bytes = b"[package]\nname = \"example\"\nversion = \"1.2.3\"\n";
    // Independently verified with both Git hash-object and Python hashlib.
    let oid = "cde0b0200916b77be7428a8db1ebf574752aa1f9";
    let mut regular = entry("100644", "blob");
    regular["sha"] = json!(oid);
    let mut responses = prefix(&regular);
    responses.push((
        route(&format!("git/blobs/{oid}")),
        json!({"sha":oid,"size":bytes.len(),"encoding":"base64","content":STANDARD.encode(bytes)}),
    ));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    let mut reader = SourceFiles::new(&api, &"a".repeat(40)).expect("source commit");
    assert_eq!(reader.read("Cargo.toml").expect("regular manifest"), bytes);
    fixture.assert_drained();
}

#[test]
fn symlinks_gitlinks_and_duplicate_entries_never_read_blob_data() {
    for (mode, kind) in [("120000", "blob"), ("160000", "commit"), ("040000", "tree")] {
        let (api, fixture) = GitHubApi::with_fixture("example/app", prefix(&entry(mode, kind)));
        let mut reader = SourceFiles::new(&api, &"a".repeat(40)).expect("source");
        assert!(reader.read("Cargo.toml").is_err());
        fixture.assert_drained();
    }
    let mut responses = prefix(&entry("100644", "blob"));
    responses[1].1["tree"] = json!([entry("100644", "blob"), entry("100644", "blob")]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(
        SourceFiles::new(&api, &"a".repeat(40))
            .expect("source")
            .read("Cargo.toml")
            .is_err()
    );
    fixture.assert_drained();
}

#[test]
fn wrong_tree_or_truncated_inventory_fails_before_blob_download() {
    for field in ["sha", "truncated"] {
        let mut responses = prefix(&entry("100644", "blob"));
        responses[1].1[field] = if field == "sha" {
            json!("d".repeat(40))
        } else {
            json!(true)
        };
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        assert!(
            SourceFiles::new(&api, &"a".repeat(40))
                .expect("source")
                .read("Cargo.toml")
                .is_err()
        );
        fixture.assert_drained();
    }
}

#[test]
fn invalid_paths_and_blob_sizes_fail_closed() {
    let responses = prefix(&entry("100644", "blob"));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses[..1].to_vec());
    let mut reader = SourceFiles::new(&api, &"a".repeat(40)).expect("source");
    for path in [
        "../Cargo.toml",
        "/Cargo.toml",
        "a//Cargo.toml",
        "a/./Cargo.toml",
        "a\\Cargo.toml",
    ] {
        assert!(reader.read(path).is_err());
    }
    fixture.assert_drained();
    let mut responses = prefix(&entry("100644", "blob"));
    responses.push((route(&format!("git/blobs/{}", "c".repeat(40))),
        json!({"sha":"c".repeat(40),"size":2,"encoding":"base64","content":STANDARD.encode(b"123")})));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(
        SourceFiles::new(&api, &"a".repeat(40))
            .expect("source")
            .read("Cargo.toml")
            .is_err()
    );
    fixture.assert_drained();
}

#[test]
fn same_length_changed_blob_cannot_reuse_expected_object_id() {
    let oid = "cde0b0200916b77be7428a8db1ebf574752aa1f9";
    let altered = b"[package]\nname = \"example\"\nversion = \"1.2.4\"\n";
    let mut regular = entry("100644", "blob");
    regular["sha"] = json!(oid);
    let mut responses = prefix(&regular);
    responses.push((route(&format!("git/blobs/{oid}")),
        json!({"sha":oid,"size":altered.len(),"encoding":"base64","content":STANDARD.encode(altered)})));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    let error = SourceFiles::new(&api, &"a".repeat(40))
        .expect("source")
        .read("Cargo.toml")
        .expect_err("object mismatch");
    assert!(
        error
            .to_string()
            .contains("release_version_blob_object_digest")
    );
    fixture.assert_drained();
}

#[test]
fn recursive_tree_uses_private_eight_mib_budget_without_raising_default_json_limit() {
    let entries = large_tree_entries(1_400);
    let tree = json!({"sha":"b".repeat(40),"truncated":false,"tree":entries.clone()});
    let bytes = serde_json::to_vec(&tree).expect("tree serialization");
    assert!(bytes.len() > 1024 * 1024);
    assert!(bytes.len() < 8 * 1024 * 1024);

    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(
            route(&format!("git/trees/{}?recursive=1", "b".repeat(40))),
            tree.clone(),
        )],
    );
    let default_result: Result<Value, _> =
        api.get_json(&api.route(&format!("git/trees/{}?recursive=1", "b".repeat(40))));
    assert!(
        default_result
            .expect_err("default API JSON limit remains one MiB")
            .to_string()
            .contains("package_publish_api_failed")
    );
    fixture.assert_drained();

    let (api, fixture) = GitHubApi::with_fixture("example/app", recursive_tree(&entries));
    let files = SourceFiles::new(&api, &"a".repeat(40))
        .expect("source commit")
        .regular_files()
        .expect("tree within its private budget");
    let mut expected: Vec<_> = (0..1_400)
        .map(|index| SourceFileEntry {
            path: large_tree_path(index),
            mode: "100644".to_owned(),
            sha: "c".repeat(40),
        })
        .collect();
    expected.sort_by(|left, right| left.path.cmp(&right.path));
    assert_eq!(files, expected);
    fixture.assert_drained();
}

#[test]
fn recursive_tree_over_eight_mib_is_rejected_before_decode() {
    let entries = large_tree_entries(9_000);
    assert!(entries.len() < MAX_SOURCE_TREE_ENTRIES);
    let tree = json!({"sha":"b".repeat(40),"truncated":false,"tree":entries.clone()});
    let bytes = serde_json::to_vec(&tree).expect("tree serialization");
    assert!(bytes.len() > 8 * 1024 * 1024);

    let (api, fixture) = GitHubApi::with_fixture("example/app", recursive_tree(&entries));
    let error = SourceFiles::new(&api, &"a".repeat(40))
        .expect("source commit")
        .regular_files()
        .expect_err("tree over private eight MiB budget");
    assert!(error.to_string().contains("package_publish_api_failed"));
    fixture.assert_drained();
}
