use super::{
    AtomicBool, CaptureAttemptInput, CleanupClaim, CompletedLaunch, Context, EffectBudget,
    ExecInspectResponse, Failure, HostError, INSPECT_INTERVAL, Instant, diagnostics,
    request_effect, within,
};

pub(super) async fn capture_attempt(
    input: &CaptureAttemptInput<'_, '_>,
    budget: &mut EffectBudget<'_>,
    mut state: diagnostics::CaptureState,
) -> Result<(diagnostics::CaptureState, Option<Vec<u8>>), Failure> {
    if state.phase == diagnostics::CapturePhase::Prepared {
        state = create_exec(input, budget, state).await?;
    }
    loop {
        if Instant::now() >= input.deadline {
            return Err(Failure::request(
                "runner log deadline",
                HostError::DockerTimeout,
            ));
        }
        if let Some(terminal) = inspect_prior_exec(input, budget, &state).await? {
            return Ok((terminal, None));
        }
        state = state
            .next_start_request()
            .map_err(|error| Failure::request("runner log start state", error))?;
        persist_state(
            input.context,
            input.launch,
            input.claim,
            budget,
            input.binding,
            &state,
        )
        .await?;
        if let Ok(archive) = start_and_collect(input, budget, &state).await {
            let inspected = wait_for_exec(input, budget, &state).await?;
            if let Some(exit_code) = inspected.exit_code {
                let terminal = save_terminal(input, budget, state, &inspected).await?;
                if matches!(exit_code, 0 | 3) {
                    let exec_id = terminal
                        .exec_id
                        .as_deref()
                        .ok_or_else(|| Failure::not_proven("runner log exec id"))?;
                    let checked =
                        diagnostics::validate_runner_log_exec_exit(exec_id, &inspected, archive)
                            .map_err(|error| Failure::request("runner log output", error))?;
                    return Ok((terminal, Some(checked)));
                }
                return Ok((terminal, None));
            }
        } else {
            let inspected = wait_for_exec(input, budget, &state).await?;
            if inspected.exit_code.is_some() {
                let terminal = save_terminal(input, budget, state, &inspected).await?;
                return Ok((terminal, None));
            }
        }
    }
}

async fn inspect_prior_exec(
    input: &CaptureAttemptInput<'_, '_>,
    budget: &mut EffectBudget<'_>,
    state: &diagnostics::CaptureState,
) -> Result<Option<diagnostics::CaptureState>, Failure> {
    if state.phase != diagnostics::CapturePhase::StartRequested {
        return Ok(None);
    }
    let inspected = inspect_exec(input, budget, state).await?;
    let terminal = if inspected.exit_code.is_some() {
        Some(inspected)
    } else if inspected.running == Some(true) {
        let finished = wait_for_exec(input, budget, state).await?;
        finished.exit_code.map(|_| finished)
    } else {
        None
    };
    match terminal {
        Some(inspected) => save_terminal(input, budget, state.clone(), &inspected)
            .await
            .map(Some),
        None => Ok(None),
    }
}

async fn create_exec(
    input: &CaptureAttemptInput<'_, '_>,
    budget: &mut EffectBudget<'_>,
    state: diagnostics::CaptureState,
) -> Result<diagnostics::CaptureState, Failure> {
    let requested = state
        .next_create_request()
        .map_err(|error| Failure::request("runner log create state", error))?;
    persist_state(
        input.context,
        input.launch,
        input.claim,
        budget,
        input.binding,
        &requested,
    )
    .await?;
    let created = request_effect(
        input.context,
        input.launch,
        input.claim,
        budget,
        "runner log exec create",
        input.identity,
        || async {
            within(input.context.docker.create_exec(
                input.identity.dind_container_id(),
                diagnostics::runner_log_create_exec_options(),
            ))
            .await?
            .map_err(|_| HostError::Docker)
        },
    )
    .await?;
    diagnostics::validate_runner_log_exec_id(&created.id)
        .map_err(|error| Failure::request("runner log exec identity", error))?;
    let created_state = requested
        .created(created.id)
        .map_err(|error| Failure::request("runner log exec state", error))?;
    persist_state(
        input.context,
        input.launch,
        input.claim,
        budget,
        input.binding,
        &created_state,
    )
    .await?;
    Ok(created_state)
}

async fn start_and_collect(
    input: &CaptureAttemptInput<'_, '_>,
    budget: &mut EffectBudget<'_>,
    state: &diagnostics::CaptureState,
) -> Result<Vec<u8>, Failure> {
    let exec_id = state
        .exec_id
        .as_deref()
        .ok_or_else(|| Failure::not_proven("runner log exec id"))?;
    let output = request_effect(
        input.context,
        input.launch,
        input.claim,
        budget,
        "runner log exec start",
        input.identity,
        || async {
            within(
                input
                    .context
                    .docker
                    .start_exec(exec_id, Some(diagnostics::runner_log_start_exec_options())),
            )
            .await?
            .map_err(|_| HostError::Docker)
        },
    )
    .await?;
    request_effect(
        input.context,
        input.launch,
        input.claim,
        budget,
        "runner log exec output",
        input.identity,
        || async {
            diagnostics::collect_runner_log_exec_output(
                output,
                input.started,
                &AtomicBool::new(false),
            )
            .await
        },
    )
    .await
}

async fn inspect_exec(
    input: &CaptureAttemptInput<'_, '_>,
    budget: &mut EffectBudget<'_>,
    state: &diagnostics::CaptureState,
) -> Result<ExecInspectResponse, Failure> {
    let exec_id = state
        .exec_id
        .as_deref()
        .ok_or_else(|| Failure::not_proven("runner log exec id"))?;
    inspect_exec_id(input, budget, exec_id).await
}

async fn inspect_exec_id(
    input: &CaptureAttemptInput<'_, '_>,
    budget: &mut EffectBudget<'_>,
    exec_id: &str,
) -> Result<ExecInspectResponse, Failure> {
    let result = request_effect(
        input.context,
        input.launch,
        input.claim,
        budget,
        "runner log exec inspect",
        input.identity,
        || async {
            within(input.context.docker.inspect_exec(exec_id))
                .await?
                .map_err(|_| HostError::Docker)
        },
    )
    .await?;
    if result.id.as_deref() != Some(exec_id)
        || result.container_id.as_deref() != Some(input.identity.dind_container_id())
        || result.running.is_none()
        || result
            .exit_code
            .is_some_and(|code| !(0..=255).contains(&code))
        || result.running == Some(true) && result.exit_code.is_some()
    {
        return Err(Failure::not_proven("runner log exec binding"));
    }
    Ok(result)
}

async fn wait_for_exec(
    input: &CaptureAttemptInput<'_, '_>,
    budget: &mut EffectBudget<'_>,
    state: &diagnostics::CaptureState,
) -> Result<ExecInspectResponse, Failure> {
    loop {
        let inspected = inspect_exec(input, budget, state).await?;
        if inspected.running == Some(false) {
            return Ok(inspected);
        }
        let remaining = input.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Failure::request(
                "runner log exec quiescence",
                HostError::DockerTimeout,
            ));
        }
        tokio::time::sleep(remaining.min(INSPECT_INTERVAL)).await;
    }
}

async fn save_terminal(
    input: &CaptureAttemptInput<'_, '_>,
    budget: &mut EffectBudget<'_>,
    state: diagnostics::CaptureState,
    inspect: &ExecInspectResponse,
) -> Result<diagnostics::CaptureState, Failure> {
    if inspect.running != Some(false) {
        return Err(Failure::not_proven("runner log exec quiescence"));
    }
    let terminal = state
        .terminal(
            inspect
                .exit_code
                .ok_or_else(|| Failure::not_proven("runner log exit code"))?,
        )
        .map_err(|error| Failure::request("runner log terminal state", error))?;
    persist_state(
        input.context,
        input.launch,
        input.claim,
        budget,
        input.binding,
        &terminal,
    )
    .await?;
    Ok(terminal)
}

pub(super) async fn persist_state(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    binding: &diagnostics::CaptureBinding,
    state: &diagnostics::CaptureState,
) -> Result<(), Failure> {
    diagnostics::persist_runner_log_state(
        &diagnostics::RunnerLogWriteContext {
            context,
            root: context.diagnostics,
            launch,
            claim,
            binding,
            started: Instant::now(),
        },
        budget,
        state,
    )
    .await
}
