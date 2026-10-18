use super::{
    canonical_identity, require_tracked_clean_input, valid_source_ref,
    valid_stage_zero_regular_file, validate_preview_source,
};
use std::{ffi::OsString, time::Duration};
use velnor_actions_contract::preview_identity::PreviewIdentity;
use velnor_actions_mise::{CheckDeadline, IsolatedCommand};

use crate::git_fixture;

fn fixture_git(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    git_fixture::command(root)
        .expect("fixture Git authority")
        .args(args)
        .output()
        .expect("fixture Git available")
}

fn init_repo(root: &std::path::Path) {
    let status = git_fixture::sterile_command()
        .arg("init")
        .arg("-q")
        .current_dir(root)
        .status()
        .expect("fixture Git available");
    assert!(status.success());
    for args in [
        ["config", "user.name", "Preview Test"],
        ["config", "user.email", "preview@example.invalid"],
    ] {
        assert!(fixture_git(root, &args).status.success());
    }
}

#[test]
fn stage_zero_parser_accepts_only_exact_regular_tracked_record() {
    let path = "scripts/build.sh";
    let valid = format!("100755 {} 0\t{path}\n", "a".repeat(40));
    assert!(valid_stage_zero_regular_file(valid.as_bytes(), path));
    let sha256 = format!("100644 {} 0\t{path}\n", "b".repeat(64));
    assert!(valid_stage_zero_regular_file(sha256.as_bytes(), path));
    for bad in [
        format!("120000 {} 0\t{path}\n", "a".repeat(40)),
        format!("100644 {} 1\t{path}\n", "a".repeat(40)),
        format!(
            "100644 {} 0\t{path}\n100644 {} 0\t{path}\n",
            "a".repeat(40),
            "b".repeat(40)
        ),
        format!("100644 {} 0\tother\n", "a".repeat(40)),
    ] {
        assert!(
            !valid_stage_zero_regular_file(bad.as_bytes(), path),
            "{bad:?}"
        );
    }
}

#[test]
fn producer_identity_and_source_ref_must_be_canonical() {
    let identity = PreviewIdentity::derive("0.6.4", &"a".repeat(40), 12).expect("identity");
    assert!(canonical_identity(&identity));
    let mut forged = identity.clone();
    forged.version.push_str(".other");
    assert!(!canonical_identity(&forged));
    assert!(valid_source_ref("refs/heads/main"));
    for reference in [
        "main",
        "refs/tags/v1.0.0",
        "refs/heads/../main",
        "refs/heads/main.lock",
        "refs/heads/branch//name",
        "refs/heads/branch@{name}",
    ] {
        assert!(!valid_source_ref(reference), "{reference}");
    }
}

#[test]
fn producer_input_must_be_tracked_clean_and_regular() {
    let repo = tempfile::tempdir().expect("repo");
    init_repo(repo.path());
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\nname='p'\n").expect("file");
    let status = fixture_git(repo.path(), &["add", "Cargo.toml"]).status;
    assert!(status.success());
    let status = fixture_git(repo.path(), &["commit", "-qm", "fixture"]).status;
    assert!(status.success());
    assert!(
        require_tracked_clean_input(repo.path(), "Cargo.toml").is_ok(),
        "clean tracked input rejected"
    );

    std::fs::write(repo.path().join("Cargo.toml"), "modified\n").expect("modify");
    assert!(require_tracked_clean_input(repo.path(), "Cargo.toml").is_err());
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\nname='p'\n").expect("restore");

    std::fs::remove_file(repo.path().join("Cargo.toml")).expect("remove");
    std::os::unix::fs::symlink("/etc/passwd", repo.path().join("Cargo.toml")).expect("link");
    assert!(require_tracked_clean_input(repo.path(), "Cargo.toml").is_err());
    // The executor's separate component/type check rejects the link before Git
    // cleanliness is considered.
    assert!(crate::check_evidence::reject_link_components(repo.path(), "Cargo.toml").is_err());
}

#[test]
fn producer_input_rejects_untracked_content_at_same_path() {
    let repo = tempfile::tempdir().expect("repo");
    init_repo(repo.path());
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\nname='p'\n").expect("file");
    // A regular file without a committed Git index entry never qualifies.
    assert!(require_tracked_clean_input(repo.path(), "Cargo.toml").is_err());
}

#[test]
fn source_validation_requires_exact_head_full_history_and_matching_count() {
    let repo = tempfile::tempdir().expect("repo");
    init_repo(repo.path());
    std::fs::write(repo.path().join("file"), "source\n").expect("file");
    for args in [vec!["add", "file"], vec!["commit", "-qm", "source"]] {
        assert!(fixture_git(repo.path(), &args).status.success());
    }
    let head = super::current_head(
        repo.path(),
        CheckDeadline::after(Duration::from_secs(10)).expect("deadline"),
    )
    .expect("head");
    let identity = PreviewIdentity::derive("0.6.4", &head, 1).expect("identity");
    assert!(
        validate_preview_source(
            repo.path(),
            &identity,
            CheckDeadline::after(Duration::from_secs(10)).expect("deadline")
        )
        .is_ok()
    );

    let incorrect_count = PreviewIdentity::derive("0.6.4", &head, 2).expect("identity");
    assert!(
        validate_preview_source(
            repo.path(),
            &incorrect_count,
            CheckDeadline::after(Duration::from_secs(10)).expect("deadline")
        )
        .is_err()
    );

    std::fs::write(repo.path().join("file"), "changed undeclared source\n")
        .expect("source mutation");
    assert!(
        validate_preview_source(
            repo.path(),
            &identity,
            CheckDeadline::after(Duration::from_secs(10)).expect("deadline")
        )
        .is_err()
    );
    std::fs::write(repo.path().join("file"), "source\n").expect("restore source");

    std::fs::write(repo.path().join(".git/shallow"), format!("{head}\n")).expect("shallow marker");
    assert!(
        validate_preview_source(
            repo.path(),
            &identity,
            CheckDeadline::after(Duration::from_secs(10)).expect("deadline")
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn isolated_producer_task_receives_fixed_handoff_without_credentials() {
    let temp = tempfile::tempdir().expect("temporary root");
    let source_sha = "a".repeat(40);
    let identity = PreviewIdentity::derive("1.2.3", &source_sha, 42).expect("identity");
    let mut preview = velnor_actions_contract::config::RustBinaryPreviewConfig {
        version_env: "JACKIN_VERSION_OVERRIDE".to_owned(),
        ..Default::default()
    };
    preview.producer.build_jobs = 3;
    let output = temp.path().join("dist/preview");
    let pairs =
        super::producer_handoff(temp.path(), &output, &preview, &identity, "refs/heads/main")
            .expect("fixed producer handoff");
    let script = concat!(
        "printf '%s\\n' ",
        "\"$VELNOR_RELEASE_SOURCE_COMMIT\" ",
        "\"$VELNOR_RELEASE_SOURCE_REF\" ",
        "\"$VELNOR_RELEASE_VERSION\" ",
        "\"$VELNOR_RELEASE_OUTPUT_DIRECTORY\" ",
        "\"$VELNOR_RELEASE_OUTPUT_MANIFEST\" ",
        "\"$VELNOR_RELEASE_COMMIT_COUNT\" ",
        "\"$CARGO_BUILD_JOBS\" ",
        "\"$JACKIN_VERSION_OVERRIDE\"; ",
        "test -z \"${GH_TOKEN+x}${GITHUB_TOKEN+x}${AWS_SECRET_ACCESS_KEY+x}\""
    );
    let command = IsolatedCommand::repo_task(
        "sh",
        vec![OsString::from("-c"), OsString::from(script)],
        &[],
    )
    .expect("isolated task command")
    .with_cwd(temp.path().to_path_buf())
    .with_env(&pairs)
    .expect("declared handoff env");
    let result = super::run_preview_task(
        &command,
        CheckDeadline::after(Duration::from_secs(5)).expect("deadline"),
    )
    .expect("task success");
    let stdout = result.stdout_text("producer test").expect("task output");
    assert!(stdout.contains(&source_sha));
    assert!(stdout.contains("refs/heads/main"));
    assert!(stdout.contains(&identity.version));
    assert!(stdout.contains(output.to_str().expect("output path")));
    assert!(stdout.contains("dist/preview/package-output.json"));
    assert!(stdout.contains("42\n3\n"));
    assert!(stdout.contains(&identity.version));
}

#[cfg(unix)]
#[test]
fn isolated_producer_task_rejects_failure_and_enforces_deadline() {
    let failing = IsolatedCommand::repo_task(
        "sh",
        vec![OsString::from("-c"), OsString::from("exit 17")],
        &[],
    )
    .expect("task command");
    assert!(
        super::run_preview_task(
            &failing,
            CheckDeadline::after(Duration::from_secs(5)).expect("deadline")
        )
        .is_err()
    );

    let sleeping = IsolatedCommand::repo_task(
        "sh",
        vec![OsString::from("-c"), OsString::from("sleep 5")],
        &[],
    )
    .expect("task command");
    let start = std::time::Instant::now();
    assert!(
        super::run_preview_task(
            &sleeping,
            CheckDeadline::after(Duration::from_millis(200)).expect("deadline")
        )
        .is_err()
    );
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "timeout should stop task"
    );
}
