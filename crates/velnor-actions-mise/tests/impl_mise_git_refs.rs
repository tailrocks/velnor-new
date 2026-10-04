#![cfg(any(target_os = "linux", target_os = "macos"))]

#[path = "impl_mise_git_refs_fixture.rs"]
mod fixture;

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use fixture::{
    git_output, git_owned, mutate_after_private_ref, raw_output, repo, set_origin_head, set_ref,
    set_symbolic_ref, unique_branch,
};
use velnor_actions_mise::{CancelHandle, GitRequest, MiseError, ProcessOutput};

const CAPTURE_LIMIT: usize = 1024 * 1024;

fn origin_head_request() -> GitRequest {
    GitRequest::rev_parse(
        ["--abbrev-ref", "origin/HEAD"]
            .into_iter()
            .map(OsString::from)
            .collect(),
    )
}

fn run_origin_head(root: &Path) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    origin_head_request().command_in(root).run_cancellable(
        CAPTURE_LIMIT,
        Duration::from_secs(10),
        &cancel,
    )
}

fn assert_native_result(root: &Path) -> Result<(), String> {
    let raw = raw_output(root, &["rev-parse", "--abbrev-ref", "origin/HEAD"])?;
    let private = run_origin_head(root).map_err(|error| error.to_string())?;
    assert_eq!(private.success, raw.status.success());
    assert_eq!(private.stdout, raw.stdout);
    assert_eq!(private.stderr, raw.stderr);
    Ok(())
}

fn assert_refusal(error: MiseError) {
    let text = error.to_string();
    assert!(
        matches!(&error, MiseError::InvalidStepInput { .. }),
        "{text}"
    );
    assert!(text.contains("ref"), "unexpected ref error: {text}");
}

#[test]
fn origin_head_preserves_ambiguous_native_namespace() -> Result<(), String> {
    let fixture = repo("refs-ambiguous", "sha1", 4)?;
    let oid = set_origin_head(&fixture.root, "main")?;
    for name in [
        "refs/heads/origin/HEAD",
        "refs/tags/origin/HEAD",
        "refs/remotes/upstream/origin/HEAD",
    ] {
        set_ref(&fixture.root, name, &oid)?;
    }
    git_owned(
        &fixture.root,
        vec![
            "config".to_owned(),
            "core.warnAmbiguousRefs".to_owned(),
            "true".to_owned(),
        ],
    )?;
    assert_native_result(&fixture.root)
}

#[test]
fn origin_head_keeps_branch_tag_remote_scopes_distinct() -> Result<(), String> {
    let fixture = repo("refs-scopes", "sha1", 4)?;
    let oid = set_origin_head(&fixture.root, "main")?;
    for name in [
        "refs/heads/main",
        "refs/tags/main",
        "refs/remotes/upstream/main",
    ] {
        set_ref(&fixture.root, name, &oid)?;
    }
    assert_native_result(&fixture.root)
}

#[test]
fn symbolic_ref_cycle_is_refused_before_private_execution() -> Result<(), String> {
    let fixture = repo("refs-cycle", "sha1", 4)?;
    set_symbolic_ref(
        &fixture.root,
        "refs/heads/velnor-cycle-a",
        "refs/heads/velnor-cycle-b",
    )?;
    set_symbolic_ref(
        &fixture.root,
        "refs/heads/velnor-cycle-b",
        "refs/heads/velnor-cycle-a",
    )?;
    assert_refusal(run_origin_head(&fixture.root).expect_err("cycle must refuse"));
    Ok(())
}

#[test]
fn source_ref_mutation_race_is_refused_when_observed() -> Result<(), String> {
    let fixture = repo("refs-mutation", "sha1", 4)?;
    let old_oid = set_origin_head(&fixture.root, "main")?;
    git_owned(
        &fixture.root,
        vec![
            "commit".to_owned(),
            "--allow-empty".to_owned(),
            "-m".to_owned(),
            "replacement".to_owned(),
        ],
    )?;
    let replacement = git_output(&fixture.root, &["rev-parse", "HEAD"])?
        .trim()
        .to_owned();
    let branch = unique_branch("ref-mutation");
    set_ref(&fixture.root, &format!("refs/heads/{branch}"), &old_oid)?;
    let watcher = mutate_after_private_ref(&fixture.root, &branch, &replacement);
    let result = run_origin_head(&fixture.root);
    let watched = watcher
        .join()
        .map_err(|_| "mutation watcher panicked".to_owned())?;
    assert!(watched.is_ok(), "mutation was not observed: {watched:?}");
    let error = result.expect_err("source mutation must refuse");
    assert!(error.to_string().contains("source_ref_namespace_changed"));
    Ok(())
}

#[test]
fn origin_head_command_scrubs_git_selectors() -> Result<(), String> {
    let fixture = repo("refs-environment", "sha1", 4)?;
    let command = origin_head_request().command_in(&fixture.root);
    let selectors = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_NAMESPACE",
    ];
    let hostile: Vec<_> = selectors
        .iter()
        .map(|key| (OsString::from(*key), OsString::from("/hostile")))
        .collect();
    let environment = command.spawn_env(&hostile);
    for key in selectors {
        assert!(
            !environment
                .iter()
                .any(|(name, value)| { name == key && value == "/hostile" })
        );
        let error = command
            .clone()
            .with_env(&[(OsString::from(key), OsString::from("/hostile"))])
            .expect_err("Git selector override must reject");
        assert!(matches!(error, MiseError::InvalidStepInput { field, .. } if field == key));
    }
    assert!(
        environment
            .iter()
            .any(|(name, value)| { name == "GIT_OPTIONAL_LOCKS" && value == "0" })
    );
    Ok(())
}
