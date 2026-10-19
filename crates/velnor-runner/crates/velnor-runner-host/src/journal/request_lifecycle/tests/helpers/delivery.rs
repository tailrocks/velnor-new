use crate::worker::execution_authorization::{
    TestControlFile, VerifiedProofChannelBinding, test_execution_challenge_publication_receipt,
};

pub(super) async fn prepare_jit_and_job_facts(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
    runner: &velnor_runner_github::RunnerReference,
    request_id: i64,
) -> Result<(), HostError> {
    use crate::launch::bootstrap::test_job_facts_publication;

    let (generation, digest) = scope.test_active_authority_identity().await?;
    let jit = scope
        .claim_runner_jit_for_request(
            provisioned.intent_id,
            provisioned.reservation_id,
            request_id,
            &runner.name,
            generation,
            &digest,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope.bind_jit_runner(jit, runner).await?;

    let publication = scope
        .claim_job_facts_publication(provisioned.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let profile = scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let proof = test_job_facts_publication(
        provisioned.reservation_id,
        provisioned.intent_id,
        TEST_ENGINE_ID.to_owned(),
        provisioned.docker_root_digest.clone(),
        provisioned.sample_generation_id.clone(),
        profile.generation_id().to_owned(),
        profile.profile_digest().to_owned(),
        provisioned.runner_container_id.clone(),
        provisioned.dind_container_id.clone(),
        runner.clone(),
        provisioned.instance_binding_digest.clone(),
        "/opt/velnor/job-facts/storage-cache-admission.json".to_owned(),
        "9".repeat(64),
        501,
        (0, 46),
        (0, 46),
        0x0102_1994,
        (0, 46, 77),
        46_u64,
        "a".repeat(64),
        true,
    )?;
    scope
        .record_job_facts_published(&publication, &proof)
        .await?;

    Ok(())
}

pub(super) async fn publish_test_execution_challenge(
    scope: &ConnectionJournal,
    channel: &VerifiedProofChannelBinding,
) -> Result<(), HostError> {
    let claim = scope
        .claim_execution_challenge(channel.reservation_id(), channel)
        .await?
        .ok_or(HostError::Journal)?;
    let control = channel.control();
    let file = TestControlFile {
        path: "/opt/velnor/source-proof/control/challenge.json".to_owned(),
        inode: control
            .directory_inode()
            .checked_add(1)
            .ok_or(HostError::Journal)?,
        bytes: u64::try_from(claim.canonical_bytes().len()).map_err(|_| HostError::Journal)?,
        control_directory_inode: control.directory_inode(),
        uid: 0,
        gid: 0,
        mode: 0o444,
        mount_id: control.mount_id(),
        device: control.device(),
        statx_device: control.statx_device(),
        fs_type_magic: control.fs_type_magic(),
        filesystem_id: control.filesystem_id(),
        filesystem_instance_digest: control.filesystem_instance_digest().to_owned(),
        mount_source: channel.observation().control.mount_source.clone(),
        read_only: control.read_only(),
        no_copy: control.no_copy(),
    };
    let receipt = test_execution_challenge_publication_receipt(&claim, channel, file)?;
    scope
        .record_execution_challenge_published(&claim, &receipt)
        .await
}

pub(super) async fn claim_listener_delivery(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
    runner: &velnor_runner_github::RunnerReference,
) -> Result<crate::journal::ListenerDeliveryClaim, HostError> {
    let claim = scope
        .claim_listener_delivery(provisioned.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if claim.engine_id() != TEST_ENGINE_ID
        || claim.docker_root_digest() != provisioned.docker_root_digest
        || claim.runner_container_id() != provisioned.runner_container_id
        || claim.dind_container_id() != provisioned.dind_container_id
        || claim.instance_binding_digest() != provisioned.instance_binding_digest
        || claim.runner() != runner
    {
        return Err(HostError::Journal);
    }
    Ok(claim)
}

pub(super) async fn finish_listener_delivery_after_challenge(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
    runner: &velnor_runner_github::RunnerReference,
) -> Result<(), HostError> {
    use crate::launch::bootstrap::test_listener_delivery_receipt;

    let delivery = claim_listener_delivery(scope, provisioned, runner).await?;
    let proof = test_listener_delivery_receipt(
        provisioned.reservation_id,
        provisioned.intent_id,
        TEST_ENGINE_ID.to_owned(),
        provisioned.docker_root_digest.clone(),
        provisioned.runner_container_id.clone(),
        runner.clone(),
    )?;
    scope.record_listener_delivered(&delivery, &proof).await
}

pub(super) async fn prepare_initialized_listener_delivery(
    scope: &ConnectionJournal,
    binding: &crate::config::RepositoryConnection,
    initialized: &JobFactsReaderFixture,
    runner: &velnor_runner_github::RunnerReference,
    request_id: i64,
) -> Result<crate::journal::ListenerDeliveryClaim, HostError> {
    use crate::config::HostConfig;
    use crate::worker::execution_authorization::test_channel_for_ready_job_facts;

    if initialized.assignment.request_id() != Some(request_id) {
        return Err(HostError::Identity);
    }
    prepare_jit_and_job_facts(scope, &initialized.provisioned, runner, request_id).await?;
    let channel = test_channel_for_ready_job_facts(
        initialized.reservation_id,
        initialized.intent_id,
        scope.connection_id().as_str().to_owned(),
        runner.runner_scale_set_id,
        request_id,
        TEST_ENGINE_ID.to_owned(),
        initialized.docker_root_digest.clone(),
        runner.id,
        runner.name.clone(),
        initialized.runner_container_id.clone(),
        initialized.dind_container_id.clone(),
        initialized.instance_binding_digest.clone(),
        &initialized.ready,
    )?;
    let config = HostConfig::test_with_job_authority_policy(binding)?;
    scope.install_job_authority_policy(&config).await?;
    publish_test_execution_challenge(scope, &channel).await?;
    claim_listener_delivery(scope, &initialized.provisioned, runner).await
}

pub(super) async fn finish_initialized_listener_delivery(
    scope: &ConnectionJournal,
    binding: &crate::config::RepositoryConnection,
    initialized: &JobFactsReaderFixture,
    runner: &velnor_runner_github::RunnerReference,
    request_id: i64,
) -> Result<(), HostError> {
    use crate::launch::bootstrap::test_listener_delivery_receipt;

    let delivery =
        prepare_initialized_listener_delivery(scope, binding, initialized, runner, request_id)
            .await?;
    let proof = test_listener_delivery_receipt(
        initialized.reservation_id,
        initialized.intent_id,
        TEST_ENGINE_ID.to_owned(),
        initialized.docker_root_digest.clone(),
        initialized.runner_container_id.clone(),
        runner.clone(),
    )?;
    scope.record_listener_delivered(&delivery, &proof).await
}
