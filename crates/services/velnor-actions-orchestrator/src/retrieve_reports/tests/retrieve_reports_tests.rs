//! Staged-report retrieval tests.
//!
//! Declared via `#[path]` from `retrieve_reports.rs` under `cfg(test)`.

use super::*;

use velnor_actions_contract::{digest_b3, task_report_id_for_task};

/// Plan expecting one matrix leg plus one task file.
fn plan_for(artifact_id: &str, task_id: &str, digest: &str) -> serde_json::Value {
    serde_json::json!({
        "matrix": {"include": [{
            "artifact_id": artifact_id,
            "matrix_key": "m-0123456789abcdef",
            "execute_task_ids": {"tasks": {"clippy": task_id}},
        }]},
        "obligations": [{"task_id": task_id, "task_digest": digest}],
    })
}

/// Expected task-file id for the one-leg plan.
fn task_file_id(digest: &str) -> String {
    task_report_id_for_task("local", "m-0123456789abcdef", digest).expect("task file id")
}

/// Create one fixture directory.
fn ensure_dir(path: &Path) {
    std::fs::create_dir_all(path).expect("fixture dir");
}

#[test]
fn retrieve_argv_names_exact_artifact() {
    let args = retrieve_args(
        7,
        "velnor-matrix-r7-a2-m-0123456789abcdef",
        Path::new("/tmp/x"),
        "o/r",
    )
    .expect("argv");
    let text: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        text,
        [
            "run",
            "download",
            "7",
            "--name",
            "velnor-matrix-r7-a2-m-0123456789abcdef",
            "--dir",
            "/tmp/x",
            "--repo",
            "o/r"
        ]
    );
    assert!(retrieve_args(7, "velnor-matrix-*", Path::new("/tmp/x"), "o/r").is_err());
    assert!(retrieve_args(7, "../escape", Path::new("/tmp/x"), "o/r").is_err());
    assert!(
        retrieve_args(
            7,
            "velnor-matrix-r7-a2-m-0123456789abcdef",
            Path::new("/tmp/x"),
            "not-a-slug",
        )
        .is_err(),
        "a malformed repo never builds an unscoped command"
    );
}

#[test]
fn missing_plan_retrieves_zero_without_spawning() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let run = dir.path().join("r7-a2");
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
}

/// The retrieve-step plan read is bounded like every other event-time
/// read: small plans parse, giant plans yield nothing (so nothing
/// downloads), and missing plans stay missing.
#[test]
fn retrieve_plan_read_is_bounded() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path().join("r7-a2");
    std::fs::create_dir_all(&run).expect("run dir");
    assert!(read_plan(&run).is_none(), "missing plan");
    std::fs::write(run.join("plan.json"), r#"{"matrix":{"include":[]}}"#).expect("plan");
    let plan = read_plan(&run).expect("small plan parses");
    assert_eq!(plan["matrix"]["include"].as_array().map(Vec::len), Some(0));
    let big = format!(
        r#"{{"matrix":{{"include":[]}},"pad":"{}"}}"#,
        "p".repeat(usize::try_from(MAX_RETRIEVE_PLAN_BYTES).expect("bound"))
    );
    std::fs::write(run.join("plan.json"), big).expect("big plan");
    assert!(
        read_plan(&run).is_none(),
        "an oversize plan parses to nothing"
    );
    assert_eq!(
        retrieve_reports_to(7, &run),
        0,
        "an oversize plan downloads nothing"
    );
}

/// The shared byte reader opens `NOFOLLOW`, validates the handle,
/// and bounds the read: every failure class reports its token.
#[test]
fn staged_bytes_cover_every_failure_class() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("f.bin");
    std::fs::write(&file, b"bytes").expect("write");
    assert_eq!(read_staged_bytes(&file, 16).expect("bytes"), b"bytes");
    assert_eq!(
        read_staged_bytes(&tmp.path().join("absent"), 16),
        Err("missing")
    );
    assert_eq!(read_staged_bytes(&file, 2), Err("oversize"));
    std::fs::create_dir(tmp.path().join("dir")).expect("dir");
    assert_eq!(
        read_staged_bytes(&tmp.path().join("dir"), 16),
        Err("unreadable")
    );
    assert_eq!(read_staged_text(&file, 16).expect("text"), "bytes");
    std::fs::write(&file, [0xff, 0xfe]).expect("binary");
    assert_eq!(read_staged_text(&file, 16), Err("unreadable"));
    #[cfg(unix)]
    {
        std::fs::write(&file, b"bytes").expect("rewrite");
        std::os::unix::fs::symlink(&file, tmp.path().join("link")).expect("link");
        assert_eq!(
            read_staged_bytes(&tmp.path().join("link"), 16),
            Err("symlink")
        );
        assert_eq!(
            read_staged_text(&tmp.path().join("link"), 16),
            Err("symlink")
        );
    }
}

/// Every staged-read failure class surfaces its explicit token.
#[test]
fn staged_tokens_cover_every_failure_class() {
    let aid = "velnor-matrix-r7-a2-m-0123456789abcdef";
    let task_id = "stack/rust/root/clippy/default";
    let digest = digest_b3(b"task");
    let plan = plan_for(aid, task_id, &digest);
    let read = |dir: &Path| {
        let mut errors = Vec::new();
        let (reports, tasks) = read_staged_reports("local", &plan, dir, &mut errors);
        (reports, tasks, errors)
    };
    // Missing leg reports explicitly.
    let tmp = tempfile::tempdir().expect("tempdir");
    let (_, _, errors) = read(tmp.path());
    assert_eq!(errors, [format!("missing_report:{aid}")]);
    // Malformed artifact ids never traverse.
    let bad = serde_json::json!({"matrix": {"include": [
        {"matrix_key": "m-0123456789abcdef"},
        {"artifact_id": "../escape"},
    ]}});
    let mut errors = Vec::new();
    let (reports, _) = read_staged_reports("local", &bad, tmp.path(), &mut errors);
    assert!(reports.is_empty());
    assert!(errors.contains(&"bad_artifact_id".to_owned()), "{errors:?}");
    assert!(
        errors.contains(&"bad_artifact_id:../escape".to_owned()),
        "{errors:?}"
    );
    // Unparsable and oversize legs report explicitly.
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join(aid).join("m-0123456789abcdef");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("matrix-report.json"), "not json").expect("bad");
    let (_, _, errors) = read(tmp.path());
    assert_eq!(errors, [format!("unparsable_report:{aid}")]);
    std::fs::write(
        home.join("matrix-report.json"),
        "x".repeat(usize::try_from(MAX_STAGED_REPORT_BYTES + 10).expect("bound")),
    )
    .expect("big");
    let (_, _, errors) = read(tmp.path());
    assert_eq!(errors, [format!("oversize_report:{aid}")]);
    // A directory payload reports unreadable, never parses.
    std::fs::remove_file(home.join("matrix-report.json")).expect("rm");
    std::fs::create_dir(home.join("matrix-report.json")).expect("dir");
    let (_, _, errors) = read(tmp.path());
    assert_eq!(errors, [format!("unreadable_report:{aid}")]);
}

/// Symlinks reject anywhere on a traversed staged path, and the nested
/// `gh` extract layout reads exactly.
#[test]
fn staged_symlinks_reject_and_nested_layout_reads() {
    let aid = "velnor-matrix-r7-a2-m-0123456789abcdef";
    let task_id = "stack/rust/root/clippy/default";
    let digest = digest_b3(b"task");
    let plan = plan_for(aid, task_id, &digest);
    let read = |dir: &Path| {
        let mut errors = Vec::new();
        let (reports, tasks) = read_staged_reports("local", &plan, dir, &mut errors);
        (reports, tasks, errors)
    };
    // Nested layout: the report reads from the exact nested path.
    let tmp = tempfile::tempdir().expect("tempdir");
    let nested = tmp.path().join(aid).join(aid).join("m-0123456789abcdef");
    std::fs::create_dir_all(&nested).expect("nested");
    std::fs::write(nested.join("matrix-report.json"), r#"{"report_id":"n"}"#).expect("report");
    let (reports, _, _) = read(tmp.path());
    assert_eq!(reports.len(), 1);
    #[cfg(unix)]
    {
        // A symlinked artifact home rejects even at a live target.
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("target");
        std::fs::create_dir_all(&target).expect("target");
        std::fs::write(target.join("matrix-report.json"), "{}").expect("report");
        std::os::unix::fs::symlink(&target, tmp.path().join(aid)).expect("link");
        let (reports, _, errors) = read(tmp.path());
        assert!(reports.is_empty());
        assert_eq!(errors, [format!("symlink_report:{aid}")]);
        // A symlinked nested parent rejects the same way.
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join(aid);
        std::fs::create_dir_all(&home).expect("home");
        std::os::unix::fs::symlink(&target, home.join(aid)).expect("link");
        let (reports, _, errors) = read(tmp.path());
        assert!(reports.is_empty());
        assert_eq!(errors, [format!("symlink_report:{aid}")]);
        // A symlinked matrix file rejects without reading.
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join(aid).join("m-0123456789abcdef");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(tmp.path().join("real.json"), "{}").expect("real");
        std::os::unix::fs::symlink(
            tmp.path().join("real.json"),
            home.join("matrix-report.json"),
        )
        .expect("link");
        let (reports, _, errors) = read(tmp.path());
        assert!(reports.is_empty());
        assert_eq!(errors, [format!("symlink_report:{aid}")]);
    }
}

/// Task files read beside their matrix file for the plan-derived
/// expectation only; every failure class reports its token.
#[test]
fn staged_task_files_cover_success_and_tokens() {
    let aid = "velnor-matrix-r7-a2-m-0123456789abcdef";
    let task_id = "stack/rust/root/clippy/default";
    let digest = digest_b3(b"task");
    let plan = plan_for(aid, task_id, &digest);
    let file_id = task_file_id(&digest);
    let read = |dir: &Path| {
        let mut errors = Vec::new();
        let (reports, tasks) = read_staged_reports("local", &plan, dir, &mut errors);
        (reports, tasks, errors)
    };
    let staged = || {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let home = tmp.path().join(aid).join("m-0123456789abcdef");
        ensure_dir(&home.join("tasks"));
        std::fs::write(home.join("matrix-report.json"), r#"{"report_id":"m"}"#).expect("report");
        (tmp, home)
    };
    // A valid task file reads beside its matrix file.
    let (tmp, home) = staged();
    std::fs::write(
        home.join("tasks").join(format!("{file_id}.json")),
        r#"{"task_report_id":"t"}"#,
    )
    .expect("task");
    let (reports, tasks, errors) = read(tmp.path());
    assert_eq!(reports.len(), 1);
    assert_eq!(tasks.len(), 1);
    assert!(errors.is_empty(), "{errors:?}");
    // A missing task file reports explicitly.
    let (tmp, _) = staged();
    let (_, _, errors) = read(tmp.path());
    assert_eq!(errors, [format!("missing_task:{file_id}")]);
    // An unparsable task file reports explicitly.
    let (tmp, home) = staged();
    std::fs::write(
        home.join("tasks").join(format!("{file_id}.json")),
        "not json",
    )
    .expect("task");
    let (_, _, errors) = read(tmp.path());
    assert_eq!(errors, [format!("unparsable_task:{file_id}")]);
    // An oversize task file reports explicitly.
    let (tmp, home) = staged();
    std::fs::write(
        home.join("tasks").join(format!("{file_id}.json")),
        "x".repeat(usize::try_from(MAX_STAGED_REPORT_BYTES + 10).expect("bound")),
    )
    .expect("task");
    let (_, _, errors) = read(tmp.path());
    assert_eq!(errors, [format!("oversize_task:{file_id}")]);
    // A directory payload reports unreadable.
    let (tmp, home) = staged();
    ensure_dir(&home.join("tasks").join(format!("{file_id}.json")));
    let (_, _, errors) = read(tmp.path());
    assert_eq!(errors, [format!("unreadable_task:{file_id}")]);
    #[cfg(unix)]
    {
        // A symlinked tasks dir rejects every expected file.
        let (tmp, home) = staged();
        let target = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&target).expect("target");
        std::fs::remove_dir(home.join("tasks")).expect("rm");
        std::os::unix::fs::symlink(&target, home.join("tasks")).expect("link");
        let (_, _, errors) = read(tmp.path());
        assert_eq!(errors, [format!("symlink_task:{file_id}")]);
        // A symlinked task file rejects without reading.
        let (tmp, home) = staged();
        std::fs::write(home.join("tasks").join("real.json"), "{}").expect("real");
        std::os::unix::fs::symlink(
            home.join("tasks").join("real.json"),
            home.join("tasks").join(format!("{file_id}.json")),
        )
        .expect("link");
        let (_, _, errors) = read(tmp.path());
        assert_eq!(errors, [format!("symlink_task:{file_id}")]);
    }
}

/// Write `plan.json` under a fresh `run_dir` and return the temp dir.
fn plan_dir(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let run = dir.path().join("r7-a2");
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join("plan.json"), body).expect("plan write");
    (dir, run)
}

/// A duplicated critical key in a staged task file records
/// explicitly instead of collapsing last-wins into evidence.
#[test]
fn staged_task_file_rejects_duplicate_keys() {
    let aid = "velnor-matrix-r7-a2-m-0123456789abcdef";
    let task_id = "stack/rust/root/clippy/default";
    let digest = digest_b3(b"task");
    let plan = plan_for(aid, task_id, &digest);
    let file_id = task_file_id(&digest);
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let home = tmp.path().join(aid).join("m-0123456789abcdef");
    ensure_dir(&home.join("tasks"));
    std::fs::write(home.join("matrix-report.json"), r#"{"report_id":"m"}"#).expect("report");
    std::fs::write(
        home.join("tasks").join(format!("{file_id}.json")),
        r#"{"task_report_id":"a","task_report_id":"b"}"#,
    )
    .expect("task");
    let mut errors = Vec::new();
    let (_, tasks) = read_staged_reports("local", &plan, tmp.path(), &mut errors);
    assert!(tasks.is_empty());
    assert_eq!(errors, [format!("unparsable_task:{file_id}")]);
}

#[test]
fn malformed_ids_skip_before_join_and_mkdir() {
    let (_dir, run) = plan_dir(
        r#"{"matrix": {"include": [
            {"artifact_id": "../escape"},
            {"artifact_id": "velnor-matrix-*"},
            {"artifact_id": ""}
        ]}}"#,
    );
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(
        !run.join("reports").exists(),
        "no directory for malformed IDs"
    );
    assert!(
        !run.parent().expect("parent").join("escape").exists(),
        "no traversal write"
    );
}

#[test]
fn oversize_and_duplicate_key_plans_retrieve_zero() {
    let big = "x".repeat(usize::try_from(MAX_STAGED_REPORT_BYTES + 1).expect("bound fits"));
    let (_dir, run) = plan_dir(&big);
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
    let (_dir, run) = plan_dir(r#"{"matrix": {"include": []}, "matrix": {}}"#);
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
}
