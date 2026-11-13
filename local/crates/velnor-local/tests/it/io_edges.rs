// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::os::unix::fs::symlink;
use std::process::{Command, Stdio};

use serde_json::json;

use crate::io::{REPO, TempRepo, changed, decoded, file, pull};

fn fast_import_commits(repo: &TempRepo, count: usize) -> String {
    let input: String = (0..count)
        .map(|number| {
            let from = if number == 0 {
                "from refs/heads/main^0\n"
            } else {
                ""
            };
            format!(
                "commit refs/heads/main\ncommitter T <t@example.invalid> {} +0000\ndata 1\nx\n{from}",
                1_700_000_000 + number
            )
        })
        .collect::<Vec<String>>()
        .concat();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&repo.dir)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    child
        .stdin
        .take()
        .unwrap_or_else(|| unreachable!("no stdin"))
        .write_all(input.as_bytes())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(child.wait().is_ok_and(|status| status.success()));
    repo.git(&["rev-parse", "main"])
}

#[tokio::test]
async fn a_compare_of_more_than_250_commits_continues_on_the_page_its_link_names() {
    let repo = TempRepo::new("io-long");
    repo.write("a.txt", b"one\n");
    let base = repo.commit("base");
    let head = fast_import_commits(&repo, 255);
    let stub = repo.stub("io-long", &json!([]));
    let path = format!("{REPO}/compare/{base}...{head}");

    let first = stub.get(&path).await;
    let next = first
        .link()
        .split(", ")
        .find(|link| link.ends_with("rel=\"next\""))
        .and_then(|link| link.strip_prefix("<http://api.localhost"))
        .and_then(|link| link.split_once('>'))
        .map(|(url, _)| url.to_owned())
        .unwrap_or_default();
    let second = stub.get(&next).await;

    let shas = |reply: &crate::support::Reply| -> Vec<String> {
        reply.json()["commits"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|commit| commit["sha"].as_str().unwrap_or_default().to_owned())
            .collect()
    };
    let (first_shas, second_shas) = (shas(&first), shas(&second));
    assert_eq!((first_shas.len(), second_shas.len()), (250, 5));
    assert!(!first_shas.iter().any(|sha| second_shas.contains(sha)));
    assert_eq!(second_shas.last(), Some(&head));
}

#[tokio::test]
async fn contents_without_ref_read_the_default_branch_not_the_checkout() {
    let c = changed("io-default");
    c.repo
        .git(&["checkout", "--quiet", "-b", "feature", &c.base]);
    let stub = c.repo.stub("io-default", &json!([]));

    let reply = stub.get(&format!("{REPO}/contents/a.txt")).await;

    assert_eq!(decoded(&reply.json()), "one\nTWO\nthree\n");
}

#[tokio::test]
async fn paths_are_literal_and_symlinks_to_files_in_the_repository_are_followed() {
    let repo = TempRepo::new("io-literal");
    repo.write(":(exclude)a.txt", b"magic\n");
    repo.write("a.txt", b"plain\n");
    symlink("a.txt", repo.dir.join("link.txt")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("../outside", repo.dir.join("out.txt")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.commit("base");
    let stub = repo.stub("io-literal", &json!([]));

    let magic = stub.get(&format!("{REPO}/contents/:(exclude)a.txt")).await;
    let followed = stub.get(&format!("{REPO}/contents/link.txt")).await.json();
    let escaping = stub.get(&format!("{REPO}/contents/out.txt")).await.json();

    assert_eq!(decoded(&magic.json()), "magic\n");
    assert_eq!(followed["type"], "file");
    assert_eq!(followed["path"], "a.txt");
    assert_eq!(decoded(&followed), "plain\n");
    assert_eq!(escaping["type"], "symlink");
    assert_eq!(escaping["target"], "../outside");
}

#[tokio::test]
async fn a_directory_listing_stops_at_1000_entries() {
    let repo = TempRepo::new("io-1000");
    for number in 0..1001 {
        repo.write(&format!("gen/f{number}.txt"), b"x\n");
    }
    repo.commit("base");
    let stub = repo.stub("io-1000", &json!([]));

    let listing = stub.get(&format!("{REPO}/contents/gen")).await.json();

    assert_eq!(listing.as_array().map(Vec::len), Some(1000));
}

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
