// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::os::unix::fs::symlink;
use std::process::{Command, Stdio};

use serde_json::json;
use velnor_local::event::NO_COMMIT;
use velnor_local::io::checkout::changes;

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
async fn a_long_compare_returns_the_newest_250_unpaged_and_chronological_pages_when_paged() {
    let repo = TempRepo::new("io-long");
    repo.write("a.txt", b"one\n");
    let base = repo.commit("base");
    let head = fast_import_commits(&repo, 255);
    let stub = repo.stub("io-long", &json!([]));
    let path = format!("{REPO}/compare/{base}...{head}");

    let unpaged = stub.get(&path).await;
    let first = stub.get(&format!("{path}?per_page=100&page=1")).await;
    let last = stub.get(&format!("{path}?per_page=100&page=3")).await;

    let shas = |reply: &crate::support::Reply| -> Vec<String> {
        reply.json()["commits"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|commit| commit["sha"].as_str().unwrap_or_default().to_owned())
            .collect()
    };
    let (unpaged_shas, first_shas, last_shas) = (shas(&unpaged), shas(&first), shas(&last));
    assert_eq!(unpaged_shas.len(), 250);
    assert_eq!(unpaged_shas.last(), Some(&head));
    assert_eq!(unpaged.link(), "");
    assert_eq!(unpaged.json()["total_commits"], 255);
    assert_eq!((first_shas.len(), last_shas.len()), (100, 55));
    assert_eq!(first_shas.get(5), unpaged_shas.first());
    assert_eq!(last_shas.last(), Some(&head));
    assert!(first.link().contains("rel=\"next\""));
}

#[tokio::test]
async fn nested_file_names_are_one_encoded_segment_in_file_links() {
    let repo = TempRepo::new("io-nested");
    repo.write("a.txt", b"one\n");
    let base = repo.commit("base");
    repo.write("src/deep/index.ts", b"x\n");
    let head = repo.commit("head");
    let stub = repo.stub("io-nested", &json!([pull(1, &base, &head)]));

    let files = stub.get(&format!("{REPO}/pulls/1/files")).await.json();

    let nested = file(&files, "src/deep/index.ts");
    assert_eq!(
        nested["raw_url"],
        format!("http://api.localhost/tailrocks/velnor-lab/raw/{head}/src%2Fdeep%2Findex.ts")
    );
    assert_eq!(
        nested["contents_url"],
        format!("http://api.localhost{REPO}/contents/src%2Fdeep%2Findex.ts?ref={head}")
    );
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
async fn paths_with_url_characters_are_percent_encoded_in_content_links() {
    let repo = TempRepo::new("io-url");
    repo.write("a b#c.txt", b"x\n");
    repo.commit("base");
    let stub = repo.stub("io-url", &json!([]));

    let entry = stub
        .get(&format!("{REPO}/contents/a%20b%23c.txt"))
        .await
        .json();

    assert_eq!(entry["name"], "a b#c.txt");
    assert!(
        entry["html_url"]
            .as_str()
            .is_some_and(|url| url.ends_with("/a%20b%23c.txt"))
    );
    assert!(
        entry["_links"]["html"]
            .as_str()
            .is_some_and(|url| url.ends_with("/a%20b%23c.txt"))
    );
    assert!(
        entry["url"]
            .as_str()
            .is_some_and(|url| url.contains("/contents/a%20b%23c.txt?"))
    );
}

#[tokio::test]
async fn a_tag_named_like_the_default_branch_does_not_shadow_the_branch() {
    let c = changed("io-tag");
    c.repo.git(&["tag", "main", &c.base]);
    let stub = c.repo.stub("io-tag", &json!([]));

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
async fn refs_and_messages_with_special_characters_survive_in_urls_and_commits() {
    let c = changed("io-special");
    c.repo.git(&["checkout", "--quiet", "-b", "feature#1+x&y"]);
    c.repo.write("a.txt", b"special\n");
    c.repo.git(&["add", "--all"]);
    c.repo
        .git(&["commit", "--quiet", "--message", "one\u{1}two\n\nbody"]);
    let stub = c.repo.stub("io-special", &json!([]));

    let contents = stub
        .get(&format!("{REPO}/contents/a.txt?ref=feature%231%2Bx%26y"))
        .await
        .json();
    let compare = stub
        .get(&format!("{REPO}/compare/{}...feature%231%2Bx%26y", c.head))
        .await
        .json();

    assert_eq!(
        contents["url"],
        format!("http://api.localhost{REPO}/contents/a.txt?ref=feature%231%2Bx%26y")
    );
    assert_eq!(compare["total_commits"], 1);
    assert_eq!(
        compare["commits"][0]["commit"]["message"],
        "one\u{1}two\n\nbody"
    );
}

#[tokio::test]
async fn a_diff_larger_than_the_limit_is_a_422_not_a_buffered_answer() {
    let repo = TempRepo::new("io-big");
    repo.write("a.txt", b"one\n");
    let base = repo.commit("base");
    repo.write("big.txt", "line of text\n".repeat(120_000).as_bytes());
    repo.commit("head");
    repo.git(&["commit", "--quiet", "--allow-empty", "--message", "later"]);
    let head = repo.git(&["rev-parse", "HEAD"]);
    let stub = repo.stub("io-big", &json!([pull(1, &base, &head)]));

    let pull_files = stub.get(&format!("{REPO}/pulls/1/files")).await;
    let compare = stub.get(&format!("{REPO}/compare/{base}...{head}")).await;

    let later_page = stub
        .get(&format!("{REPO}/compare/{base}...{head}?per_page=1&page=2"))
        .await;

    assert_eq!((pull_files.status, compare.status), (422, 422));
    assert_eq!(later_page.status, 200);
    assert_eq!(later_page.json()["files"], json!([]));
    assert_eq!(pull_files.json()["errors"][0]["code"], "too_large");
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

#[tokio::test]
async fn a_pushed_branch_lists_its_own_commits_and_a_merge_its_first_parent_diff() {
    let repo = TempRepo::new("push-changes");
    repo.write("base.txt", b"base");
    repo.commit("base");
    repo.git(&["switch", "--quiet", "--create", "feature"]);
    repo.write("feature.txt", b"feature");
    let feature = repo.commit("feature");
    repo.git(&["switch", "--quiet", "main"]);
    repo.write("main.txt", b"main");
    let before = repo.commit("main");
    let new = changes(&repo.dir, "feature", NO_COMMIT, &feature)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(new.len(), 1);
    assert_eq!(new[0].added, ["feature.txt"]);
    repo.git(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "merge",
        "feature",
    ]);
    let merge = repo.git(&["rev-parse", "HEAD"]);
    let merged = changes(&repo.dir, "main", &before, &merge)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let ids: Vec<&str> = merged.iter().map(|commit| commit.id.as_str()).collect();
    assert_eq!(ids, [feature.as_str(), merge.as_str()]);
    assert_eq!(merged[1].added, ["feature.txt"]);
}
