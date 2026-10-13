use super::stage_tests::{Fake, fake_container_id, fake_network_id};
use crate::HostError;
use crate::stage::{
    Forget, PairSink, PairStartPhase, PairStop, RunnerStartRequirement, drive, drive_with_profile,
    drive_with_profile_and_sink_for_test, drive_with_profile_for_test,
};
use crate::worker::WorkerNetworkPlan;
use velnor_runner_docker_spec::resolve_runner_profile;

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
async fn sink_without_durable_start_callback_is_rejected() {
    assert_eq!(
        PairSink::before_runner_start(
            &Forget,
            &fake_container_id(1),
            RunnerStartRequirement::DurableRequired,
        )
        .await,
        Err(HostError::Config)
    );
    assert_eq!(
        PairSink::before_runner_start(
            &Forget,
            &fake_container_id(1),
            RunnerStartRequirement::LegacyCompatible,
        )
        .await,
        Ok(())
    );
}

#[tokio::test]
async fn profile_bridge_is_persisted_before_create_and_dind_joins_it() -> Result<(), HostError> {
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
    let network = WorkerNetworkPlan::for_worker("worker_a")?;
    let engine = Fake::new();

    let partial = drive_with_profile_for_test(
        &engine,
        "worker_a",
        b"jit-canary",
        PairStop::DindCreated,
        &engine,
        &profile,
    )
    .await?;

    assert_eq!(
        partial.dind_id.as_deref(),
        Some(fake_container_id(1).as_str())
    );
    assert_eq!(
        engine.events(),
        [
            "sink-volume",
            "sink-network-intent",
            "create-network",
            "sink-network-id",
            "volumes",
            "create",
            "sink-dind",
        ]
    );
    assert_eq!(
        engine.specs()[0].network_mode.as_deref(),
        Some(network.name())
    );
    Ok(())
}

#[tokio::test]
async fn uncertain_network_id_persistence_does_not_create_worker_containers()
-> Result<(), HostError> {
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
    let engine = Fake::new();
    engine.fail_on("network-id");

    let result = drive_with_profile_for_test(
        &engine,
        "worker_a",
        b"jit-canary",
        PairStop::Jit,
        &engine,
        &profile,
    )
    .await;

    assert_eq!(result, Err(HostError::Docker));
    assert_eq!(
        engine.events(),
        ["sink-volume", "sink-network-intent", "create-network",]
    );
    assert_eq!(engine.specs(), Vec::new());
    Ok(())
}

#[tokio::test]
async fn uncertain_network_create_keeps_name_intent_without_cleanup() -> Result<(), HostError> {
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
    let engine = Fake::new();
    engine.fail_on("create-network");

    let failure = drive_with_profile_and_sink_for_test(
        &engine,
        "worker_a",
        b"jit-canary",
        PairStop::Jit,
        &engine,
        &profile,
    )
    .await
    .expect_err("uncertain create must stop before containers");

    assert_eq!(failure.phase(), PairStartPhase::NetworkCreation);
    assert!(failure.side_effect_may_have_succeeded());
    assert_eq!(failure.partial().outer_network_id, None);
    assert_eq!(engine.events(), ["sink-volume", "sink-network-intent"]);
    assert_eq!(engine.specs(), Vec::new());
    assert_eq!(engine.removed(), Vec::<String>::new());
    Ok(())
}

#[tokio::test]
async fn network_inspect_failure_persists_returned_id_and_stops() -> Result<(), HostError> {
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
    let engine = Fake::new();
    engine.fail_on("network-inspect");

    let failure = drive_with_profile_and_sink_for_test(
        &engine,
        "worker_a",
        b"jit-canary",
        PairStop::Jit,
        &engine,
        &profile,
    )
    .await
    .expect_err("failed inspect leaves a created bridge unresolved");

    assert_eq!(failure.phase(), PairStartPhase::NetworkCreation);
    assert!(failure.side_effect_may_have_succeeded());
    assert_eq!(
        failure.partial().outer_network_id.as_deref(),
        Some(fake_network_id().as_str())
    );
    assert_eq!(
        engine.events(),
        [
            "sink-volume",
            "sink-network-intent",
            "create-network",
            "sink-network-id"
        ]
    );
    assert_eq!(engine.specs(), Vec::new());
    assert_eq!(engine.removed(), Vec::<String>::new());
    Ok(())
}

#[tokio::test]
async fn legacy_start_keeps_default_sink_behavior_and_jit_boundary() -> Result<(), HostError> {
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
async fn journal_sink_records_intent_before_legacy_runner_start() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(
        &engine,
        "worker_a",
        b"jit",
        PairStop::RunnerStarted,
        &engine,
    )
    .await?;
    assert!(partial.runner_id.is_some());
    assert_eq!(
        engine.events(),
        [
            "sink-volume",
            "volumes",
            "create",
            "sink-dind",
            "start",
            "create",
            "sink-runner",
            "sink-runner-start-legacy",
            "start",
        ]
    );
    Ok(())
}

#[tokio::test]
async fn failed_start_intent_preserves_pair_without_starting_runner_or_writing_jit() {
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")
        .expect("pinned profile resolves for deterministic test");
    let engine = Fake::new();
    engine.fail_on("runner-start-intent");

    let failure = drive_with_profile_and_sink_for_test(
        &engine,
        "worker_a",
        b"jit-canary",
        PairStop::Jit,
        &engine,
        &profile,
    )
    .await;

    let failure = failure.expect_err("missing durable start intent must fail closed");
    assert_eq!(failure.error(), HostError::Docker);
    assert_eq!(failure.phase(), PairStartPhase::RunnerStartIntent);
    assert!(failure.side_effect_may_have_succeeded());
    assert_eq!(
        engine.events(),
        [
            "sink-volume",
            "sink-network-intent",
            "create-network",
            "sink-network-id",
            "volumes",
            "create",
            "sink-dind",
            "create",
            "sink-runner",
            "start",
        ]
    );
    assert_eq!(
        failure.partial().outer_network_id.as_deref(),
        Some(fake_network_id().as_str())
    );
    assert_eq!(engine.removed(), Vec::<String>::new());
}

#[tokio::test]
async fn profile_path_uses_official_pinned_runner_and_private_dind() -> Result<(), HostError> {
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
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
    assert!(partial.dind_id.is_some());
    assert!(partial.runner_id.is_some());
    assert_eq!(
        engine.events(),
        [
            "sink-volume",
            "sink-network-intent",
            "create-network",
            "sink-network-id",
            "volumes",
            "create",
            "sink-dind",
            "create",
            "sink-runner",
            "start",
            "sink-runner-start-durable",
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
        Some(format!("container:{}", fake_container_id(1)).as_str())
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
async fn profile_runner_start_failure_retains_network_and_pair_identity() -> Result<(), HostError> {
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
    let engine = Fake::new();
    engine.fail_on_nth("start", 2);

    let failure = drive_with_profile_and_sink_for_test(
        &engine,
        "worker_a",
        b"jit-canary",
        PairStop::Jit,
        &engine,
        &profile,
    )
    .await
    .expect_err("runner start error is ambiguous");

    assert_eq!(failure.phase(), PairStartPhase::RunnerStart);
    assert!(failure.side_effect_may_have_succeeded());
    assert_eq!(
        failure.partial().outer_network_id.as_deref(),
        Some(fake_network_id().as_str())
    );
    assert_eq!(
        failure.partial().dind_id.as_deref(),
        Some(fake_container_id(1).as_str())
    );
    assert_eq!(
        failure.partial().runner_id.as_deref(),
        Some(fake_container_id(2).as_str())
    );
    assert_eq!(engine.removed(), Vec::<String>::new());
    Ok(())
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
