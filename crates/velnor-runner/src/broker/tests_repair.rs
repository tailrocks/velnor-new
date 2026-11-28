// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::net::TcpListener;
use std::time::Duration;

use super::{Broker, FailedStep, Request, Response, TrustedFacts, Window};

fn facts(server: &str) -> TrustedFacts {
    facts_at(server, &"a".repeat(40))
}

fn facts_at(server: &str, sha: &str) -> TrustedFacts {
    TrustedFacts::parse(
        format!(
            r#"{{"repository_id": 1, "repository": "tailrocks/velnor-new",
"server_url": "{server}", "default_branch": "main", "sha": "{sha}", "run_id": 1,
"run_attempt": 1, "event_name": "push", "event": {{"kind": "branch", "branch": "main"}}}}"#
        )
        .as_bytes(),
    )
    .expect("facts")
}

#[tokio::test]
async fn a_repair_that_outlives_its_cap_reports_failure_and_stops_git() {
    let silent = TcpListener::bind("127.0.0.1:0").expect("listen");
    let server = format!(
        "http://127.0.0.1:{}",
        silent.local_addr().expect("addr").port()
    );
    let root = std::env::temp_dir().join(format!("velnor-cap-{}", std::process::id()));
    let broker = Broker::new(facts(&server), root.clone()).with_repair_cap(Duration::from_secs(1));
    let request = Request::RepairMirror {
        token: None,
        window: Window::COMMIT,
        failed: FailedStep::CheckoutIndex,
    };
    let response =
        tokio::time::timeout(Duration::from_secs(60), broker.handle(&mut None, request)).await;
    let _ = std::fs::remove_dir_all(&root);
    match response.map(|reply| reply.response) {
        Ok(Response::Failed { message }) => {
            assert!(message.contains("longer than 1 seconds"), "{message}");
        }
        other => panic!("the cap did not end the repair: {other:?}"),
    }
}

#[tokio::test]
async fn the_cap_does_not_count_the_wait_for_the_mirror_lock() {
    let root = std::env::temp_dir().join(format!("velnor-cap-lock-{}", std::process::id()));
    let facts = facts("file:///nonexistent-velnor-server");
    let mirror = super::mirror_path(&root, &facts).expect("mirror path");
    let mut lock_file = mirror.clone().into_os_string();
    lock_file.push(".lock");
    std::fs::create_dir_all(mirror.parent().expect("parent")).expect("parent dir");
    let held = std::fs::File::create(&lock_file).expect("lock file");
    held.lock().expect("take the lock");
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(11));
        drop(held);
    });
    let broker = Broker::new(facts, root.clone()).with_repair_cap(Duration::from_secs(10));
    let request = Request::RepairMirror {
        token: None,
        window: Window::COMMIT,
        failed: FailedStep::CheckoutIndex,
    };
    let response =
        tokio::time::timeout(Duration::from_secs(120), broker.handle(&mut None, request)).await;
    release.join().expect("release the lock");
    let _ = std::fs::remove_dir_all(&root);
    match response.map(|reply| reply.response) {
        Ok(Response::Failed { message }) => {
            assert!(!message.contains("took longer than"), "{message}");
        }
        other => panic!("the repair did not fail on the missing server: {other:?}"),
    }
}

#[tokio::test]
async fn a_fetch_that_outlives_its_cap_reports_failure_stops_git_and_keeps_the_marker() {
    let silent = TcpListener::bind("127.0.0.1:0").expect("listen");
    let server = format!(
        "http://127.0.0.1:{}",
        silent.local_addr().expect("addr").port()
    );
    let root = std::env::temp_dir().join(format!("velnor-fetch-cap-{}", std::process::id()));
    let facts = facts(&server);
    let mirror = super::mirror_path(&root, &facts).expect("mirror path");
    let broker = Broker::new(facts, root.clone()).with_fetch_cap(Duration::from_secs(1));
    let request = Request::FetchMirror { token: None };
    let response =
        tokio::time::timeout(Duration::from_secs(60), broker.handle(&mut None, request)).await;
    let marked = mirror.join("sweep-needed").exists();
    let survivors = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(root.display().to_string())
        .status()
        .expect("pgrep");
    let _ = std::fs::remove_dir_all(&root);
    match response.map(|reply| reply.response) {
        Ok(Response::Failed { message }) => {
            assert!(
                message.contains("fetching into the mirror took longer than 1 seconds"),
                "{message}"
            );
        }
        other => panic!("the cap did not end the fetch: {other:?}"),
    }
    assert!(marked, "the interrupted fetch left no marker");
    assert!(!survivors.success(), "a git of the fetch survived the cap");
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[tokio::test]
async fn a_pipe_where_an_object_belongs_is_cut_off_by_the_cap_and_swept_by_the_next_fetch() {
    let base = std::env::temp_dir().join(format!("velnor-pipe-{}", std::process::id()));
    let work = base.join("work");
    let upstream = base.join("up/tailrocks/velnor-new.git");
    std::fs::create_dir_all(&work).expect("work");
    std::fs::create_dir_all(&upstream).expect("upstream");
    git(&upstream, &["init", "-q", "--bare"]);
    git(&work, &["init", "-q", "-b", "main"]);
    std::fs::write(work.join("file"), "x").expect("file");
    git(&work, &["add", "file"]);
    git(&work, &["commit", "-q", "-m", "first"]);
    git(
        &work,
        &["push", "-q", &upstream.display().to_string(), "main"],
    );
    let sha = git(&work, &["rev-parse", "HEAD"]);
    let tree = git(&work, &["rev-parse", "HEAD^{tree}"]);
    let server = format!("file://{}", base.join("up").display());
    let root = base.join("store");
    let fetch = |cap: u64| {
        let broker = Broker::new(facts_at(&server, &sha), root.clone())
            .with_fetch_cap(Duration::from_secs(cap));
        async move {
            broker
                .handle(&mut None, Request::FetchMirror { token: None })
                .await
                .response
        }
    };
    assert!(matches!(fetch(60).await, Response::Done { .. }));
    let mirror = super::mirror_path(&root, &facts_at(&server, &sha)).expect("mirror path");
    let object = mirror.join("objects").join(&tree[..2]).join(&tree[2..]);
    assert!(object.is_file(), "the tree is not a loose object");
    std::fs::remove_file(&object).expect("remove the tree");
    let made = std::process::Command::new("mkfifo")
        .arg(&object)
        .status()
        .expect("mkfifo");
    assert!(made.success());
    let stuck = tokio::time::timeout(Duration::from_secs(60), fetch(3)).await;
    let survivors = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(base.display().to_string())
        .status()
        .expect("pgrep");
    let recovered = tokio::time::timeout(Duration::from_secs(60), fetch(60)).await;
    let swept = object.symlink_metadata().map_or(true, |meta| {
        !std::os::unix::fs::FileTypeExt::is_fifo(&meta.file_type())
    });
    let _ = std::fs::remove_dir_all(&base);
    match stuck {
        Ok(Response::Failed { message }) => {
            assert!(message.contains("took longer than 3 seconds"), "{message}");
        }
        other => panic!("the cap did not end the fetch: {other:?}"),
    }
    assert!(!survivors.success(), "a git of the fetch survived the cap");
    assert!(
        matches!(recovered, Ok(Response::Done { .. })),
        "{recovered:?}"
    );
    assert!(swept, "the pipe is still where the object belongs");
}
