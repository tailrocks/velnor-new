/// Execute idempotent, exact-owner cleanup for a known launch generation.
///
/// The ledger must reject generations that are not fenced by a durable worker
/// identity. Unknown acquire/registration/JIT effects are not resolved here.
///
/// # Errors
///
/// Returns an error when identity, diagnostics, ownership, or absence cannot be
/// proven. The ledger retains the last durable checkpoint for reconciliation.
pub async fn cleanup_worker_generation<E: WorkerCleanupEngine, L: CleanupLedger>(
    engine: &E,
    ledger: &L,
    diagnostics_store: &DiagnosticsStore,
    identity: WorkerGenerationIdentity,
    post_actions: PostActionDisposition,
    stop_policy: RunnerStopPolicy,
) -> Result<WorkerTerminationProof, HostError> {
    validate_request(&identity, &post_actions, &stop_policy)?;
    ledger.begin(&identity, &post_actions, &stop_policy).await?;

    let initial = engine.inspect_generation(&identity).await?;
    let runner_start = reconcile_runner_start(ledger, &identity, initial.runner()).await?;
    if matches!(post_actions, PostActionDisposition::NotRun)
        && runner_start != velnor_runner_journal::journal::RunnerStartObservation::NeverStarted
    {
        return Err(HostError::Identity);
    }
    let prior_children = ledger.prior_children_drained(&identity).await?;
    if !initial.dind().present && prior_children.is_none() {
        return Err(HostError::Docker);
    }
    let runner_stop = stop_runner_if_running(
        engine,
        ledger,
        &identity,
        &stop_policy,
        initial.runner().running,
    )
    .await?;
    let diagnostics =
        retain_diagnostics(engine, ledger, diagnostics_store, &identity, &post_actions).await?;
    let children =
        drain_children(engine, ledger, &identity, initial.dind(), prior_children).await?;
    stop_dind_after_children(engine, ledger, &identity).await?;
    let absent_containers = remove_outer_containers(engine, ledger, &identity).await?;
    let outer_network_absent = remove_outer_network(engine, ledger, &identity).await?;
    let absent_volumes = remove_owned_volumes(engine, ledger, &identity).await?;

    let proof = WorkerTerminationProof {
        identity,
        outer_network_absent,
        runner_start_observation: runner_start,
        post_actions,
        diagnostics,
        children,
        absent_containers,
        absent_volumes,
        runner_forced: runner_stop.forced,
    };
    ledger.complete(&proof).await?;
    Ok(proof)
}

async fn reconcile_runner_start<L: CleanupLedger>(
    ledger: &L,
    identity: &WorkerGenerationIdentity,
    runner: ContainerObservation,
) -> Result<velnor_runner_journal::journal::RunnerStartObservation, HostError> {
    use velnor_runner_journal::journal::RunnerStartObservation as Start;

    let prior = ledger.runner_start_observation(identity).await?;
    match (prior, runner.present, runner.started) {
        (Some(Start::NeverStarted), true, Some(Start::MayHaveStarted) | None)
        | (None, true, None)
        | (None, false, _) => Err(HostError::Identity),
        (Some(observed), _, _) => Ok(observed),
        (None, true, Some(observed)) => {
            ledger
                .record_runner_start_observation(identity, observed)
                .await?;
            Ok(observed)
        }
    }
}

async fn stop_runner_if_running<E: WorkerCleanupEngine, L: CleanupLedger>(
    engine: &E,
    ledger: &L,
    identity: &WorkerGenerationIdentity,
    stop_policy: &RunnerStopPolicy,
    running: bool,
) -> Result<RunnerStopEvidence, HostError> {
    let step = CleanupStep::RunnerTermination;
    ledger.before(identity.launch_id(), &step).await?;
    if !running {
        let stopped = RunnerStopEvidence {
            stopped: true,
            forced: false,
        };
        ledger.after(identity.launch_id(), &step).await?;
        return Ok(stopped);
    }
    let stopped = engine.stop_runner(identity, stop_policy).await?;
    if !stopped.stopped {
        return Err(HostError::Docker);
    }
    ledger.after(identity.launch_id(), &step).await?;
    Ok(stopped)
}

async fn stop_dind_after_children<E: WorkerCleanupEngine, L: CleanupLedger>(
    engine: &E,
    ledger: &L,
    identity: &WorkerGenerationIdentity,
) -> Result<DindStopEvidence, HostError> {
    let step = CleanupStep::DindTermination;
    ledger.before(identity.launch_id(), &step).await?;
    let stopped = engine.stop_dind(identity).await?;
    if !stopped.stopped {
        return Err(HostError::Docker);
    }
    ledger.after(identity.launch_id(), &step).await?;
    Ok(stopped)
}

async fn retain_diagnostics<E: WorkerCleanupEngine, L: CleanupLedger>(
    engine: &E,
    ledger: &L,
    store: &DiagnosticsStore,
    identity: &WorkerGenerationIdentity,
    post_actions: &PostActionDisposition,
) -> Result<DiagnosticsReceipt, HostError> {
    let step = CleanupStep::DiagnosticsRetention;
    ledger.before(identity.launch_id(), &step).await?;
    let diagnostics = if let Some(receipt) = store.load(identity, post_actions)? {
        receipt
    } else {
        let archive = engine.runner_diagnostics(identity).await?;
        store.retain(
            identity,
            post_actions,
            archive.as_ref().map(|bytes| bytes.as_slice()),
        )?
    };
    ledger
        .diagnostics(identity.launch_id(), &diagnostics)
        .await?;
    ledger.after(identity.launch_id(), &step).await?;
    Ok(diagnostics)
}

async fn drain_children<E: WorkerCleanupEngine, L: CleanupLedger>(
    engine: &E,
    ledger: &L,
    identity: &WorkerGenerationIdentity,
    dind: ContainerObservation,
    prior_children: Option<ChildCleanupEvidence>,
) -> Result<ChildCleanupEvidence, HostError> {
    if let Some(evidence) = prior_children {
        if dind.present && dind.running {
            let remaining = engine.list_dind_children(identity).await?;
            if !remaining.containers.is_empty() || !remaining.networks.is_empty() {
                return Err(HostError::Docker);
            }
            let step = CleanupStep::ChildrenDrained;
            ledger.before(identity.launch_id(), &step).await?;
            ledger.children_drained(identity, &evidence).await?;
            ledger.after(identity.launch_id(), &step).await?;
        }
        return Ok(evidence);
    }
    if !dind.present {
        return Err(HostError::Docker);
    }
    if !dind.running {
        return if dind.started
            == Some(velnor_runner_journal::journal::RunnerStartObservation::NeverStarted)
        {
            record_never_started_dind_empty_children(ledger, identity).await
        } else {
            Err(HostError::Docker)
        };
    }
    if dind.started != Some(velnor_runner_journal::journal::RunnerStartObservation::MayHaveStarted)
    {
        return Err(HostError::Identity);
    }
    let inventory_step = CleanupStep::ChildEnumeration;
    ledger.before(identity.launch_id(), &inventory_step).await?;
    let mut inventory = engine.list_dind_children(identity).await?;
    inventory.containers.sort_unstable();
    inventory.containers.dedup();
    inventory.networks.sort_unstable();
    inventory.networks.dedup();
    ledger.observe_children(identity, &inventory).await?;
    ledger.after(identity.launch_id(), &inventory_step).await?;
    remove_child_resources(
        engine,
        ledger,
        identity,
        &inventory.containers,
        ChildResourceKind::Container,
    )
    .await?;
    remove_child_resources(
        engine,
        ledger,
        identity,
        &inventory.networks,
        ChildResourceKind::Network,
    )
    .await?;
    let remaining = engine.list_dind_children(identity).await?;
    if !remaining.containers.is_empty() || !remaining.networks.is_empty() {
        return Err(HostError::Docker);
    }
    let mut evidence = ledger.observed_children(identity).await?;
    evidence.container_ids.sort_unstable();
    evidence.container_ids.dedup();
    evidence.network_ids.sort_unstable();
    evidence.network_ids.dedup();
    let drained = CleanupStep::ChildrenDrained;
    ledger.before(identity.launch_id(), &drained).await?;
    ledger.children_drained(identity, &evidence).await?;
    ledger.after(identity.launch_id(), &drained).await?;
    Ok(evidence)
}

async fn record_never_started_dind_empty_children<L: CleanupLedger>(
    ledger: &L,
    identity: &WorkerGenerationIdentity,
) -> Result<ChildCleanupEvidence, HostError> {
    let inventory = ChildResourceInventory {
        containers: Vec::new(),
        networks: Vec::new(),
    };
    let enumeration = CleanupStep::ChildEnumeration;
    ledger.before(identity.launch_id(), &enumeration).await?;
    ledger.observe_children(identity, &inventory).await?;
    ledger.after(identity.launch_id(), &enumeration).await?;

    let evidence = ledger.observed_children(identity).await?;
    if !evidence.container_ids.is_empty() || !evidence.network_ids.is_empty() {
        return Err(HostError::Identity);
    }
    let drained = CleanupStep::ChildrenDrained;
    ledger.before(identity.launch_id(), &drained).await?;
    ledger.children_drained(identity, &evidence).await?;
    ledger.after(identity.launch_id(), &drained).await?;
    Ok(evidence)
}

async fn remove_child_resources<E: WorkerCleanupEngine, L: CleanupLedger>(
    engine: &E,
    ledger: &L,
    identity: &WorkerGenerationIdentity,
    ids: &[String],
    kind: ChildResourceKind,
) -> Result<(), HostError> {
    for id in ids {
        let step = CleanupStep::ChildResourceRemoval {
            id: id.clone(),
            kind,
        };
        ledger.before(identity.launch_id(), &step).await?;
        engine.remove_dind_child(identity, id, kind).await?;
        ledger.after(identity.launch_id(), &step).await?;
    }
    Ok(())
}

async fn remove_outer_containers<E: WorkerCleanupEngine, L: CleanupLedger>(
    engine: &E,
    ledger: &L,
    identity: &WorkerGenerationIdentity,
) -> Result<Vec<String>, HostError> {
    let mut absent = Vec::with_capacity(2);
    for role in [OuterContainerRole::Runner, OuterContainerRole::Dind] {
        let (step, id) = match role {
            OuterContainerRole::Runner => {
                (CleanupStep::RunnerRemoval, identity.runner_container_id())
            }
            OuterContainerRole::Dind => (CleanupStep::DindRemoval, identity.dind_container_id()),
        };
        ledger.before(identity.launch_id(), &step).await?;
        engine.remove_outer_container(identity, role).await?;
        ledger.after(identity.launch_id(), &step).await?;
        absent.push(id.to_owned());
    }
    Ok(absent)
}

async fn remove_outer_network<E: WorkerCleanupEngine, L: CleanupLedger>(
    engine: &E,
    ledger: &L,
    identity: &WorkerGenerationIdentity,
) -> Result<bool, HostError> {
    match (identity.outer_network_name(), identity.outer_network_id()) {
        (None, None) => Ok(true),
        (Some(_), Some(_)) => {
            let step = CleanupStep::OuterNetworkRemoval;
            ledger.before(identity.launch_id(), &step).await?;
            engine.remove_outer_network(identity).await?;
            ledger.after(identity.launch_id(), &step).await?;
            Ok(true)
        }
        _ => Err(HostError::Identity),
    }
}

async fn remove_owned_volumes<E: WorkerCleanupEngine, L: CleanupLedger>(
    engine: &E,
    ledger: &L,
    identity: &WorkerGenerationIdentity,
) -> Result<Vec<String>, HostError> {
    let step = CleanupStep::VolumeRemoval;
    ledger.before(identity.launch_id(), &step).await?;
    let mut absent = engine.remove_named_volumes(identity).await?;
    absent.sort_unstable();
    let mut expected = super::volumes::worker_volume_names(identity.worker_volume())?;
    expected.sort_unstable();
    if absent != expected {
        return Err(HostError::Docker);
    }
    ledger.after(identity.launch_id(), &step).await?;
    Ok(absent)
}

fn validate_request(
    identity: &WorkerGenerationIdentity,
    post_actions: &PostActionDisposition,
    stop_policy: &RunnerStopPolicy,
) -> Result<(), HostError> {
    if matches!(post_actions, PostActionDisposition::Completed)
        && matches!(stop_policy, RunnerStopPolicy::StopAtDeadline { .. })
    {
        return Err(HostError::Identity);
    }
    if let RunnerStopPolicy::StopAtDeadline {
        grace_seconds,
        reason_class,
    } = stop_policy
        && (*grace_seconds == 0 || !valid_reason_class(reason_class))
    {
        return Err(HostError::Identity);
    }
    if let PostActionDisposition::Interrupted { reason_class } = post_actions
        && !valid_reason_class(reason_class)
    {
        return Err(HostError::Identity);
    }
    if matches!(post_actions, PostActionDisposition::NotRun) && identity.observed_job().is_some() {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn valid_runner_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_worker_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_reason_class(reason: &str) -> bool {
    !reason.is_empty()
        && reason.len() <= 64
        && reason
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}
