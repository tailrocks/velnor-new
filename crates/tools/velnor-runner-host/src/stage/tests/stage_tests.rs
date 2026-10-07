//! Stage transitions and cleanup decisions exercised with a deterministic fake engine.

use super::super::{
    Forget, PairStop, decide, drive, drive_with_profile, drive_with_profile_for_test,
};
use super::{Fake, fake_id};
use crate::HostError;
use velnor_runner_docker_spec::resolve_runner_profile;

#[tokio::test]
async fn dind_created_does_not_start() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::DindCreated, &Forget).await?;
    assert_eq!(partial.dind_id, Some(fake_id(1)));
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes", "create"]);
    Ok(())
}

#[tokio::test]
async fn jit_stop_writes_stdin_after_both_starts() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::Jit, &Forget).await?;
    assert_eq!(partial.dind_id, Some(fake_id(1)));
    assert_eq!(partial.runner_id, Some(fake_id(2)));
    assert_eq!(
        engine.events(),
        ["volumes", "create", "start", "create", "start", "jit"]
    );
    Ok(())
}

#[tokio::test]
async fn remove_recorded_deletes_only_the_owned_id() -> Result<(), HostError> {
    let engine = Fake::new();
    engine
        .names
        .lock()
        .map_err(|_| HostError::Docker)?
        .insert("runner".to_owned(), "aaaaaaaaaaaa".to_owned());
    let decision = decide(&engine, "aaaaaaaaaaaa", "runner").await?;
    assert_eq!(decision, crate::DeleteDecision::Delete);
    assert_eq!(engine.events(), ["remove"]);
    Ok(())
}

#[tokio::test]
async fn remove_recorded_keeps_a_foreign_id() -> Result<(), HostError> {
    let engine = Fake::new();
    engine
        .names
        .lock()
        .map_err(|_| HostError::Docker)?
        .insert("runner".to_owned(), "bbbbbbbbbbbb".to_owned());
    let decision = decide(&engine, "aaaaaaaaaaaa", "runner").await?;
    assert_eq!(decision, crate::DeleteDecision::KeepForeign);
    assert_eq!(engine.events(), Vec::<&str>::new());
    Ok(())
}

#[tokio::test]
async fn volumes_stop_creates_no_container() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::Volumes, &Forget).await?;
    assert_eq!(partial.dind_id, None);
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes"]);
    Ok(())
}

#[tokio::test]
async fn dind_started_does_not_create_the_runner() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::DindStarted, &Forget).await?;
    assert_eq!(partial.dind_id, Some(fake_id(1)));
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes", "create", "start"]);
    Ok(())
}

#[tokio::test]
async fn runner_created_does_not_start() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(
        &engine,
        "worker_a",
        b"jit",
        PairStop::RunnerCreated,
        &Forget,
    )
    .await?;
    assert_eq!(partial.dind_id, Some(fake_id(1)));
    assert_eq!(partial.runner_id, Some(fake_id(2)));
    assert_eq!(engine.events(), ["volumes", "create", "start", "create"]);
    Ok(())
}

#[tokio::test]
async fn profile_path_uses_official_pinned_runner_and_private_dind() -> Result<(), HostError> {
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
    let dind_id = fake_id(1);
    let dind_network_mode = format!("container:{dind_id}");
    let engine = Fake::new();
    let partial = drive_with_profile_for_test(
        &engine,
        "worker_a",
        b"jit-canary-not-a-job-env",
        PairStop::Jit,
        &engine,
        &profile,
    )
    .await?;
    assert_eq!(partial.dind_id.as_deref(), Some(dind_id.as_str()));
    assert_eq!(partial.runner_id, Some(fake_id(2)));
    assert_eq!(
        engine.events(),
        [
            "sink-volume",
            "volumes",
            "create",
            "sink-dind",
            "create",
            "sink-runner",
            "start",
            "start",
            "jit",
        ]
    );
    let specs = engine.specs();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].image, profile.dind_image());
    assert!(specs[0].privileged);
    assert_eq!(specs[0].cmd.first().map(String::as_str), Some("dockerd"));
    assert_eq!(specs[1].image, profile.runner_image());
    assert!(!specs[1].privileged);
    assert_eq!(
        specs[1].security_opts,
        vec!["apparmor=velnor-runner".to_owned()]
    );
    assert_eq!(specs[1].group_add, ["2375".to_owned()]);
    assert_eq!(
        specs[1].network_mode.as_deref(),
        Some(dind_network_mode.as_str())
    );
    assert!(specs[1].readonly_rootfs);
    assert_eq!(specs[1].mounts[2].target, "/home/runner/externals");
    assert!(specs[1].mounts[2].read_only);
    assert!(specs.iter().all(|spec| {
        !spec
            .env
            .iter()
            .any(|entry| entry.contains("jit-canary-not-a-job-env"))
            && !spec
                .cmd
                .iter()
                .any(|entry| entry.contains("jit-canary-not-a-job-env"))
            && !spec
                .labels
                .iter()
                .any(|entry| entry.contains("jit-canary-not-a-job-env"))
    }));

    Ok(())
}

#[tokio::test]
async fn profile_path_fails_closed_before_side_effects_without_approved_policy() {
    let Ok(profile) = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set") else {
        panic!("the pinned image profile must resolve for this test");
    };
    let engine = Fake::new();
    let result = drive_with_profile(
        &engine,
        "worker_a",
        b"jit-canary",
        PairStop::Jit,
        &engine,
        &profile,
    )
    .await;
    assert_eq!(result, Err(HostError::Config));
    assert_eq!(engine.events(), Vec::<&'static str>::new());
}

#[tokio::test]
async fn runner_started_does_not_write_jit() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(
        &engine,
        "worker_a",
        b"jit",
        PairStop::RunnerStarted,
        &Forget,
    )
    .await?;
    assert!(partial.runner_id.is_some());
    assert_eq!(
        engine.events(),
        ["volumes", "create", "start", "create", "start"]
    );
    Ok(())
}

#[tokio::test]
async fn second_create_failure_removes_only_the_owned_dind() -> Result<(), HostError> {
    let engine = Fake::new();
    *engine.fail_at.lock().map_err(|_| HostError::Docker)? = Some(("create", 2));
    engine
        .names
        .lock()
        .map_err(|_| HostError::Docker)?
        .insert("foreign".to_owned(), "bbbbbbbbbbbb".to_owned());
    let Err(error) = drive(&engine, "worker_a", b"jit", PairStop::Jit, &Forget).await else {
        return Err(HostError::Docker);
    };
    assert_eq!(error, HostError::Docker);
    assert_eq!(engine.removed(), [fake_id(1)]);
    assert_eq!(engine.events(), ["volumes", "create", "start", "remove"]);
    let names = engine.names.lock().map_err(|_| HostError::Docker)?;
    assert_eq!(
        names.get("foreign").map(String::as_str),
        Some("bbbbbbbbbbbb")
    );
    Ok(())
}

#[tokio::test]
async fn missing_name_is_not_deleted() -> Result<(), HostError> {
    let engine = Fake::new();
    let decision = decide(&engine, "aaaaaaaaaaaa", "absent").await?;
    assert_eq!(decision, crate::DeleteDecision::NotDeleted);
    assert_eq!(engine.events(), Vec::<&str>::new());
    Ok(())
}
