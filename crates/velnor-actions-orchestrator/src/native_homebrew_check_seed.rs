use super::{BTreeMap, Path, SeedTransitionAdmission, WorkflowEvent};

pub(crate) fn derive_seed_transition(
    root: &Path,
    repository: &str,
    event: WorkflowEvent,
    pull_request: Option<u64>,
    expected_head: &str,
    deadline: velnor_actions_mise::CheckDeadline,
    required: bool,
) -> Result<Option<SeedTransitionAdmission>, crate::OrchestratorError> {
    if event != WorkflowEvent::PullRequest {
        return no_seed_match(required);
    }
    let Some(context) = seed_pull_context(root, repository, pull_request, expected_head, deadline)?
    else {
        return no_seed_match(required);
    };
    admit_seed_transition(root, repository, event, expected_head, context, deadline).map(Some)
}

fn no_seed_match<T>(required: bool) -> Result<Option<T>, crate::OrchestratorError> {
    if required {
        Err(crate::internal::internal(
            "homebrew_adoption_transition_not_admitted",
        ))
    } else {
        Ok(None)
    }
}

struct SeedPullContext {
    number: u64,
    repository_id: u64,
    pull: crate::package_release_publish::PublicPullRequestSnapshot,
    transition: crate::homebrew::adoption::ProtectedHomebrewAdoptionTransition,
}

fn seed_pull_context(
    root: &Path,
    repository: &str,
    pull_request: Option<u64>,
    expected_head: &str,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<Option<SeedPullContext>, crate::OrchestratorError> {
    use crate::internal::internal;

    let Some(transition_id) = std::env::var("VELNOR_HOMEBREW_ADOPTION_TRANSITION_ID")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let Some(number) = pull_request.filter(|number| *number > 0) else {
        return Ok(None);
    };
    let repository_id =
        crate::package_release_publish::read_public_repository_identity(repository, deadline)?;
    let pull = crate::package_release_publish::read_public_pull_request(
        repository,
        repository_id,
        number,
        deadline,
    )?;
    let protected_transition =
        crate::homebrew::adoption::ProtectedHomebrewAdoptionTransition::from_protected_source(
            include_bytes!("../../../.velnor/qualification/homebrew/jackin/transition.json"),
        )?;
    if !seed_pull_context_matches(
        protected_transition.transition(),
        &transition_id,
        repository,
        repository_id,
        &pull,
        expected_head,
    ) {
        return Ok(None);
    }
    crate::package_release_publish::verify_public_pull_request_lineage(
        repository,
        repository_id,
        "main",
        &pull.base_sha,
        &pull.head_sha,
        deadline,
    )?;
    let transition = crate::homebrew::adoption::protected_homebrew_adoption_transition(
        repository,
        repository_id,
        &pull.base_sha,
    )?;
    if crate::homebrew::authority::probe_protected_homebrew_authority(
        root,
        repository,
        WorkflowEvent::PullRequest,
        Some(number),
        expected_head,
        deadline,
    )?
    .is_some()
    {
        return Ok(None);
    }
    if transition.transition().transition_id != transition_id {
        return Err(internal("homebrew_adoption_transition_identity"));
    }
    Ok(Some(SeedPullContext {
        number,
        repository_id,
        pull,
        transition,
    }))
}

fn admit_seed_transition(
    root: &Path,
    repository: &str,
    event: WorkflowEvent,
    expected_head: &str,
    context: SeedPullContext,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<SeedTransitionAdmission, crate::OrchestratorError> {
    use crate::internal::internal;

    let version = std::env::var("VELNOR_HOMEBREW_ADOPTION_VERSION")
        .map_err(|_| internal("sdk_release_selection_missing"))?;
    let source_sha = std::env::var("VELNOR_HOMEBREW_ADOPTION_SOURCE_SHA")
        .map_err(|_| internal("sdk_release_selection_missing"))?;
    let manifest_sha256 = std::env::var("VELNOR_HOMEBREW_ADOPTION_MANIFEST_SHA256")
        .map_err(|_| internal("sdk_release_selection_missing"))?;
    let binding = crate::homebrew::sdk_binding::verify_published_sdk_binding(
        &context.transition,
        &version,
        &source_sha,
        &manifest_sha256,
        deadline,
    )?;
    let run_id = positive_env_u64("GITHUB_RUN_ID")?;
    let run_attempt = u32::try_from(positive_env_u64("GITHUB_RUN_ATTEMPT")?)
        .map_err(|_| internal("sdk_adoption_caller_run_invalid"))?;
    crate::package_release_publish::verify_public_sdk_adoption_caller(
        &crate::package_release_publish::PublicSdkCallerSelection {
            repository: repository.to_owned(),
            repository_id: context.repository_id,
            head_repository: context.pull.head_repository.clone(),
            head_repository_id: context.pull.head_repository_id,
            pull_request_number: context.number,
            pull_request_base_sha: context.pull.base_sha.clone(),
            pull_request_head_sha: context.pull.head_sha.clone(),
            run_id,
            run_attempt,
            sdk_source_sha: source_sha.clone(),
        },
        deadline,
    )?;
    let (evidence, sdk) = render_and_validate_seed(
        root,
        repository,
        event,
        expected_head,
        &context,
        &binding,
        deadline,
    )?;
    Ok(SeedTransitionAdmission {
        transition: context.transition,
        evidence,
        sdk,
    })
}

fn render_and_validate_seed(
    root: &Path,
    repository: &str,
    event: WorkflowEvent,
    expected_head: &str,
    context: &SeedPullContext,
    binding: &crate::homebrew::sdk_binding::VerifiedPublishedSdkBinding,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<
    (
        velnor_actions_contract::homebrew_adoption::HomebrewAdoptionEvidence,
        crate::homebrew::sdk_binding::PublishedSdkBindingIdentity,
    ),
    crate::OrchestratorError,
> {
    use crate::internal::internal;

    let sdk = binding.identity().clone();
    let seed_pins = crate::product_release_pins::homebrew_adoption_pins()?;
    let generated = velnor_actions_workflow_renderer::render_homebrew_adoption_seed(
        context.transition.transition(),
        &velnor_actions_workflow_renderer::HomebrewAdoptionSeedSdkIdentity {
            source_sha: &sdk.source_sha,
            version: &sdk.version,
            manifest_bytes: binding.manifest_bytes(),
            manifest_sha256: &sdk.manifest_sha256,
            host_target_sha256: &sdk.seed_runner_binary_sha256,
        },
        &seed_pins,
    )
    .map_err(|_| internal("homebrew_adoption_seed_render_invalid"))?
    .into_iter()
    .map(|file| (file.path, file.bytes.into_bytes()))
    .collect::<BTreeMap<_, _>>();
    let evidence = crate::homebrew::adoption::validate_homebrew_adoption(
        &crate::homebrew::adoption::AdoptionValidationContext {
            root,
            admitted: &context.transition,
            tap_repository: repository,
            tap_repository_id: context.repository_id,
            event,
            authenticated_base_sha: &context.pull.base_sha,
            authenticated_head_sha: expected_head,
            deadline,
        },
        &generated,
    )?;
    Ok((evidence, sdk))
}

fn positive_env_u64(name: &str) -> Result<u64, crate::OrchestratorError> {
    use crate::internal::internal;

    let value = std::env::var(name).map_err(|_| internal("sdk_adoption_caller_run_missing"))?;
    let parsed = value
        .parse::<u64>()
        .map_err(|_| internal("sdk_adoption_caller_run_invalid"))?;
    if parsed == 0 {
        return Err(internal("sdk_adoption_caller_run_invalid"));
    }
    Ok(parsed)
}

pub(crate) fn seed_pull_context_matches(
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    requested_transition_id: &str,
    repository: &str,
    repository_id: u64,
    pull: &crate::package_release_publish::PublicPullRequestSnapshot,
    expected_head: &str,
) -> bool {
    transition.transition_id == requested_transition_id
        && transition.tap_repository == repository
        && transition.tap_repository_id == repository_id
        && transition.base_sha == pull.base_sha
        && pull.state == "open"
        && !pull.draft
        && !pull.merged
        && pull.base_repository == repository
        && pull.base_repository_id == repository_id
        && pull.base_ref == "main"
        && pull.head_repository == repository
        && pull.head_repository_id == repository_id
        && pull.head_sha == expected_head
}

pub(crate) fn validate_seed_transition(
    root: &Path,
    repository: &str,
    event: WorkflowEvent,
    pull_request: Option<u64>,
    expected_head: &str,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<SeedTransitionAdmission, crate::OrchestratorError> {
    derive_seed_transition(
        root,
        repository,
        event,
        pull_request,
        expected_head,
        deadline,
        true,
    )?
    .ok_or_else(|| crate::internal::internal("homebrew_adoption_transition_not_admitted"))
}
