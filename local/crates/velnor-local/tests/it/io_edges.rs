// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use crate::io::{REPO, TempRepo, changed, file, pull};

#[tokio::test]
async fn a_revision_that_looks_like_an_option_or_a_path_that_climbs_out_is_not_found() {
    let c = changed("io-hostile");
    let outside = c.repo.dir.with_extension("outside");
    std::fs::write(&outside, "secret\n").unwrap_or_else(|e| unreachable!("{e}"));
    let written = c.repo.dir.with_extension("written");
    let option = format!("--output={}", written.display());
    let stub = c
        .repo
        .stub("io-hostile", &json!([pull(1, &option, &c.head)]));
    let climb = format!(
        "../{}",
        outside.file_name().unwrap_or_default().to_string_lossy()
    );

    let pull_files = stub.get(&format!("{REPO}/pulls/1/files")).await;
    let compare = stub
        .get(&format!("{REPO}/compare/{option}...{}", c.head))
        .await;
    let reference = stub
        .get(&format!("{REPO}/contents/a.txt?ref={option}"))
        .await;
    let dash = stub.get(&format!("{REPO}/contents/a.txt?ref=-x")).await;
    let climbing = stub.get(&format!("{REPO}/contents/{climb}")).await;
    let encoded = stub
        .get(&format!(
            "{REPO}/contents/sub%2F..%2F..%2F{}",
            climb.trim_start_matches("../")
        ))
        .await;

    for reply in [pull_files, compare, reference, dash, climbing, encoded] {
        assert_eq!(reply.status, 404);
    }
    assert!(!written.exists());
}

#[tokio::test]
async fn file_names_that_git_quotes_are_returned_as_they_are_on_disk() {
    let repo = TempRepo::new("io-quoted");
    repo.write("we\"ird\tname.txt", b"one\n");
    repo.write("moved \"a\".txt", b"keep\nkeep\nkeep\nkeep\n");
    let base = repo.commit("base");
    repo.write("we\"ird\tname.txt", b"two\n");
    repo.git(&["mv", "moved \"a\".txt", "moved \"b\".txt"]);
    let head = repo.commit("head");
    let stub = repo.stub("io-quoted", &json!([pull(1, &base, &head)]));

    let files = stub.get(&format!("{REPO}/pulls/1/files")).await.json();

    assert_eq!(file(&files, "we\"ird\tname.txt")["status"], "modified");
    let renamed = file(&files, "moved \"b\".txt");
    assert_eq!(renamed["status"], "renamed");
    assert_eq!(renamed["previous_filename"], "moved \"a\".txt");
}

#[tokio::test]
async fn a_submodule_is_a_file_in_a_listing_and_a_submodule_when_asked_for_directly() {
    let repo = TempRepo::new("io-submodule");
    repo.write("sub/keep.txt", b"keep\n");
    repo.commit("base");
    let target = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{target},sub/mod"),
    ]);
    repo.git(&["commit", "--quiet", "--message", "submodule"]);
    let stub = repo.stub("io-submodule", &json!([]));

    let listing = stub.get(&format!("{REPO}/contents/sub")).await.json();
    let direct = stub.get(&format!("{REPO}/contents/sub/mod")).await.json();

    let types: Vec<(&str, &str)> = listing
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| {
            (
                entry["name"].as_str().unwrap_or_default(),
                entry["type"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(types, [("keep.txt", "file"), ("mod", "file")]);
    assert_eq!(direct["type"], "submodule");
}
