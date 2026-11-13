// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::process::Command;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use crate::support::{Stub, temp_dir};

const REPO: &str = "/repos/tailrocks/velnor-lab";

#[derive(Debug)]
struct TempRepo {
    dir: PathBuf,
}

impl TempRepo {
    fn new(name: &str) -> Self {
        let repo = Self {
            dir: temp_dir(name),
        };
        repo.git(&["init", "--quiet", "--initial-branch=main"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_AUTHOR_DATE", "2026-01-02T03:04:05Z")
            .env("GIT_COMMITTER_DATE", "2026-01-02T03:04:05Z")
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn write(&self, path: &str, content: &[u8]) {
        let full = self.dir.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| unreachable!("{e}"));
        }
        std::fs::write(full, content).unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--message", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn stub(&self, name: &str, pulls: &Value) -> Stub {
        let state = temp_dir(&format!("{name}-state"));
        std::fs::write(state.join("pulls.json"), pulls.to_string())
            .unwrap_or_else(|e| unreachable!("{e}"));
        Stub::new(&self.dir, &state, &temp_dir(&format!("{name}-cache")))
    }
}

fn pull(number: u64, base: &str, head: &str) -> Value {
    json!({"number": number, "state": "open", "base": {"sha": base}, "head": {"sha": head}})
}

struct Changed {
    base: String,
    head: String,
    repo: TempRepo,
}

fn changed(name: &str) -> Changed {
    let repo = TempRepo::new(name);
    repo.write("a.txt", b"one\ntwo\n");
    repo.write("b.txt", b"gone\n");
    repo.write("c.txt", b"rename me\nline2\nline3\nline4\n");
    repo.write("sub/d.txt", b"deep\n");
    let base = repo.commit("base");
    repo.write("a.txt", b"one\nTWO\nthree\n");
    repo.write("new.txt", b"fresh\n");
    repo.write("blob.bin", b"\0\x01\x02");
    repo.git(&["rm", "--quiet", "b.txt"]);
    repo.git(&["mv", "c.txt", "e.txt"]);
    let head = repo.commit("head");
    Changed { base, head, repo }
}

fn file<'a>(files: &'a Value, name: &str) -> &'a Value {
    files
        .as_array()
        .into_iter()
        .flatten()
        .find(|file| file["filename"] == name)
        .unwrap_or(&Value::Null)
}

#[tokio::test]
async fn pull_request_files_come_from_the_two_commits_in_github_json_shape() {
    let c = changed("io-pull-files");
    let stub = c
        .repo
        .stub("io-pull-files", &json!([pull(7, &c.base, &c.head)]));
    let new_sha = c.repo.git(&["rev-parse", "HEAD:new.txt"]);
    let old_sha = c.repo.git(&["rev-parse", &format!("{}:b.txt", c.base)]);

    let reply = stub.get(&format!("{REPO}/pulls/7/files")).await;
    let files = reply.json();
    let missing = stub.get(&format!("{REPO}/pulls/8/files")).await;

    assert_eq!(reply.status, 200);
    assert_eq!(reply.link(), "");
    assert_eq!(files.as_array().map(Vec::len), Some(5));
    let modified = file(&files, "a.txt");
    assert_eq!(modified["status"], "modified");
    assert_eq!(
        (
            &modified["additions"],
            &modified["deletions"],
            &modified["changes"]
        ),
        (&json!(2), &json!(1), &json!(3))
    );
    assert_eq!(
        modified["patch"],
        "@@ -1,2 +1,3 @@\n one\n-two\n+TWO\n+three"
    );
    assert_eq!(
        modified["blob_url"],
        format!(
            "http://api.localhost/tailrocks/velnor-lab/blob/{}/a.txt",
            c.head
        )
    );
    assert_eq!(
        modified["raw_url"],
        format!(
            "http://api.localhost/tailrocks/velnor-lab/raw/{}/a.txt",
            c.head
        )
    );
    assert_eq!(
        modified["contents_url"],
        format!(
            "http://api.localhost/repos/tailrocks/velnor-lab/contents/a.txt?ref={}",
            c.head
        )
    );
    let added = file(&files, "new.txt");
    assert_eq!(
        (&added["status"], &added["sha"]),
        (&json!("added"), &json!(new_sha))
    );
    let removed = file(&files, "b.txt");
    assert_eq!(
        (&removed["status"], &removed["sha"]),
        (&json!("removed"), &json!(old_sha))
    );
    assert_eq!(removed["deletions"], 1);
    let renamed = file(&files, "e.txt");
    assert_eq!(renamed["status"], "renamed");
    assert_eq!(renamed["previous_filename"], "c.txt");
    assert_eq!(renamed["changes"], 0);
    assert!(renamed.get("patch").is_none());
    let binary = file(&files, "blob.bin");
    assert_eq!(binary["status"], "added");
    assert!(binary.get("patch").is_none());
    assert_eq!(missing.status, 404);
}

#[tokio::test]
async fn a_pull_request_with_more_files_than_a_page_carries_a_link_header() {
    let c = changed("io-pull-pages");
    let stub = c
        .repo
        .stub("io-pull-pages", &json!([pull(7, &c.base, &c.head)]));
    let path = format!("{REPO}/pulls/7/files");

    let first = stub.get(&format!("{path}?per_page=2")).await;
    let second = stub.get(&format!("{path}?per_page=2&page=2")).await;
    let last = stub.get(&format!("{path}?per_page=2&page=3")).await;

    let url = |page: u32| format!("<http://api.localhost{path}?page={page}&per_page=2>");
    assert_eq!(first.json().as_array().map(Vec::len), Some(2));
    assert_eq!(
        first.link(),
        format!("{}; rel=\"next\", {}; rel=\"last\"", url(2), url(3))
    );
    assert_eq!(
        second.link(),
        format!(
            "{}; rel=\"prev\", {}; rel=\"next\", {}; rel=\"last\", {}; rel=\"first\"",
            url(1),
            url(3),
            url(3),
            url(1)
        )
    );
    assert_eq!(last.json().as_array().map(Vec::len), Some(1));
    assert_eq!(
        last.link(),
        format!("{}; rel=\"prev\", {}; rel=\"first\"", url(2), url(1))
    );
}

fn many_files(name: &str, count: usize) -> (Changed, Stub) {
    let repo = TempRepo::new(name);
    repo.write("keep.txt", b"keep\n");
    let base = repo.commit("base");
    for number in 0..count {
        repo.write(&format!("gen/f{number}.txt"), b"x\n");
    }
    let head = repo.commit("head");
    let stub = repo.stub(name, &json!([pull(1, &base, &head)]));
    (Changed { base, head, repo }, stub)
}

#[tokio::test]
async fn a_pull_request_lists_at_most_3000_files_and_a_compare_at_most_300() {
    let (c, stub) = many_files("io-caps", 3001);

    let first = stub
        .get(&format!("{REPO}/pulls/1/files?per_page=100"))
        .await;
    let last = stub
        .get(&format!("{REPO}/pulls/1/files?per_page=100&page=30"))
        .await;
    let beyond = stub
        .get(&format!("{REPO}/pulls/1/files?per_page=100&page=31"))
        .await;
    let compare = stub
        .get(&format!("{REPO}/compare/{}...{}", c.base, c.head))
        .await
        .json();

    assert!(first.link().contains("page=30&per_page=100>; rel=\"last\""));
    assert_eq!(last.json().as_array().map(Vec::len), Some(100));
    assert_eq!(beyond.json().as_array().map(Vec::len), Some(0));
    assert_eq!(compare["files"].as_array().map(Vec::len), Some(300));
    assert_eq!(compare["total_commits"], 1);
}

#[tokio::test]
async fn compare_reports_commits_status_and_files_between_two_refs() {
    let c = changed("io-compare");
    let side = {
        c.repo.git(&["checkout", "--quiet", "-b", "side", &c.base]);
        c.repo.write("side.txt", b"side\n");
        c.repo.commit("side")
    };
    let stub = c.repo.stub("io-compare", &json!([]));

    let ahead = stub
        .get(&format!("{REPO}/compare/{}...{}", c.base, c.head))
        .await;
    let identical = stub
        .get(&format!("{REPO}/compare/{}...{}", c.head, c.head))
        .await
        .json();
    let diverged = stub
        .get(&format!("{REPO}/compare/{side}...{}", c.head))
        .await
        .json();
    let behind = stub
        .get(&format!("{REPO}/compare/{}...{}", c.head, c.base))
        .await
        .json();
    let by_name = stub
        .get(&format!("{REPO}/compare/main...side"))
        .await
        .json();
    let unknown = stub.get(&format!("{REPO}/compare/{}...nope", c.base)).await;
    let paged = stub
        .get(&format!(
            "{REPO}/compare/{}...{}?per_page=1&page=2",
            c.base, c.head
        ))
        .await;
    let body = ahead.json();

    assert_eq!(ahead.status, 200);
    assert_eq!(body["status"], "ahead");
    assert_eq!(
        (
            &body["ahead_by"],
            &body["behind_by"],
            &body["total_commits"]
        ),
        (&json!(1), &json!(0), &json!(1))
    );
    assert_eq!(body["base_commit"]["sha"], c.base);
    assert_eq!(body["merge_base_commit"]["sha"], c.base);
    assert_eq!(body["commits"][0]["sha"], c.head);
    assert_eq!(body["commits"][0]["commit"]["message"], "head\n");
    assert_eq!(body["commits"][0]["commit"]["author"]["name"], "Test");
    assert_eq!(body["commits"][0]["parents"][0]["sha"], c.base);
    assert_eq!(file(&body["files"], "a.txt")["status"], "modified");
    assert_eq!(identical["status"], "identical");
    assert_eq!(diverged["status"], "diverged");
    assert_eq!(
        (&diverged["ahead_by"], &diverged["behind_by"]),
        (&json!(1), &json!(1))
    );
    assert_eq!(diverged["merge_base_commit"]["sha"], c.base);
    assert_eq!(behind["status"], "behind");
    assert_eq!(behind["behind_by"], 1);
    assert_eq!(by_name["status"], "diverged");
    assert_eq!(by_name["commits"][0]["sha"], side);
    assert_eq!(unknown.status, 404);
    assert_eq!(paged.json()["commits"], json!([]));
    assert_eq!(paged.json()["files"], json!([]));
}

fn decoded(entry: &Value) -> String {
    let content: String = entry["content"]
        .as_str()
        .unwrap_or_default()
        .split('\n')
        .collect();
    String::from_utf8_lossy(&STANDARD.decode(content).unwrap_or_default()).into_owned()
}

#[tokio::test]
async fn contents_are_read_from_the_commit_named_by_ref() {
    let c = changed("io-contents");
    let stub = c.repo.stub("io-contents", &json!([]));
    let blob = c.repo.git(&["rev-parse", &format!("{}:a.txt", c.base)]);

    let old = stub
        .get(&format!("{REPO}/contents/a.txt?ref={}", c.base))
        .await;
    let new = stub
        .get(&format!("{REPO}/contents/a.txt?ref={}", c.head))
        .await;
    let nested = stub
        .get(&format!("{REPO}/contents/sub/d.txt?ref={}", c.head))
        .await;
    let directory = stub
        .get(&format!("{REPO}/contents/sub?ref={}", c.head))
        .await;
    let removed = stub
        .get(&format!("{REPO}/contents/b.txt?ref={}", c.head))
        .await;
    let was = stub
        .get(&format!("{REPO}/contents/b.txt?ref={}", c.base))
        .await;
    let bad_ref = stub.get(&format!("{REPO}/contents/a.txt?ref=nope")).await;
    let head_default = stub.get(&format!("{REPO}/contents/a.txt")).await;

    assert_eq!(old.status, 200);
    assert_eq!(old.json()["type"], "file");
    assert_eq!(old.json()["encoding"], "base64");
    assert_eq!(old.json()["sha"], blob);
    assert_eq!(old.json()["size"], 8);
    assert_eq!(old.json()["path"], "a.txt");
    assert_eq!(decoded(&old.json()), "one\ntwo\n");
    assert_eq!(decoded(&new.json()), "one\nTWO\nthree\n");
    assert_eq!(decoded(&head_default.json()), "one\nTWO\nthree\n");
    assert_eq!(nested.json()["name"], "d.txt");
    assert_eq!(directory.json()[0]["path"], "sub/d.txt");
    assert_eq!(directory.json()[0]["type"], "file");
    assert!(directory.json()[0].get("content").is_none());
    assert_eq!(removed.status, 404);
    assert_eq!(removed.json()["message"], "Not Found");
    assert_eq!(decoded(&was.json()), "gone\n");
    assert_eq!(bad_ref.status, 404);
}
