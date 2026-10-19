pub(super) async fn prepare_listener_delivery(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
    runner: &velnor_runner_github::RunnerReference,
) -> Result<crate::journal::ListenerDeliveryClaim, HostError> {
    use crate::launch::bootstrap::test_job_facts_publication;

    let jit = scope
        .claim_runner_jit(provisioned.intent_id, &runner.name)
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

pub(super) async fn finish_listener_delivery(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
    runner: &velnor_runner_github::RunnerReference,
) -> Result<(), HostError> {
    use crate::launch::bootstrap::test_listener_delivery_receipt;

    let delivery = prepare_listener_delivery(scope, provisioned, runner).await?;
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
