use std::time::SystemTime;

use crate::{
    ActionsServiceRouteLookup, AsyncDiscoveryTransport, AsyncScopedDiscoveryIntentStore,
    SessionError, WireError, exchange_organization_discovery_admin_once_async,
    issue_organization_discovery_token_async, organization_admin_evidence,
    read_organization_runner_group_policy_evidence_async, read_repository_admin_evidence_async,
};

use super::super::types::{PolicyGap, PolicyMismatch, PoolBindingView};
use super::organization::{
    organization_scope, valid_expected, validate_group_policy, validate_repository,
};
use super::{
    PoolAdmissionEvidence, read_repository_pool_trust_async, verify_organization_pool_policy,
};

/// Perform a bounded, read-only organization pool-policy preflight.
///
/// `expected` must be made from one validated host configuration byte snapshot;
/// its policy digest must be the digest of those exact protected bytes. Host
/// configuration and secret values stay outside this crate. `actions_read_token`
/// is the host-only Actions:read PAT; `controller_token` is the
/// host-only organization registration-management PAT. They may be the same
/// value when that PAT has the union of required scopes. Neither may be the
/// Scale Set service token or a workflow credential. The host transport must
/// provide fixed-origin HTTPS requests, no
/// redirects/retries, streaming response limits, and per-request deadlines.
/// This function has no single outer deadline; callers should impose one around
/// the whole future.
///
/// The function performs repository and organization policy GETs, then exactly
/// two journaled auth-bootstrap POSTs, followed by same-scope Actions Service
/// group and Set GETs. The POSTs only issue a short-lived registration token
/// and exchange it for a read-only admin connection. No runner, group, or Set
/// is created. A `Verified` result remains insufficient for job admission
/// without the state-owned fenced population and per-offer trust gates.
///
/// # Errors
///
/// Returns secret-safe transport, authorization, malformed-response, or
/// durable-intent errors. Every error is fail-closed; no Set create fallback
/// or job-side effect is attempted.
pub async fn preflight_organization_pool_admission_async<T, I>(
    transport: &mut T,
    expected: PoolBindingView<'_>,
    actions_read_token: &str,
    controller_token: &str,
    intents: &mut I,
) -> Result<PoolAdmissionEvidence, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
    I: AsyncScopedDiscoveryIntentStore + ?Sized,
{
    let Some(organization) = organization_scope(expected.registration_scope) else {
        return Ok(PoolAdmissionEvidence::Unknown(
            PolicyGap::EffectiveRoutingApplicabilityUnproven,
        ));
    };
    if let Some(evidence) = validate_preflight_input(
        &expected,
        organization,
        actions_read_token,
        controller_token,
    ) {
        return Ok(evidence);
    }

    let (owner, repository) = repository_parts(expected.target_repository_full_name)
        .ok_or(WireError::RegistrationRejected)?;
    let repository_admin =
        read_repository_admin_evidence_async(transport, owner, repository, actions_read_token)
            .await?;
    if expected
        .target_repository_id
        .is_some_and(|id| id != repository_admin.repository_id())
    {
        return Ok(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        ));
    }
    let organization_admin = organization_admin_evidence(repository_admin, organization)?;

    let repository_trust =
        read_repository_pool_trust_async(transport, owner, repository, actions_read_token).await?;
    if let Err(evidence) = validate_repository(&expected, &repository_trust) {
        return Ok(evidence);
    }

    let Some(group_policy) = read_organization_runner_group_policy_evidence_async(
        transport,
        organization,
        expected.actions_runner_group_name,
        actions_read_token,
    )
    .await?
    else {
        return Ok(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        ));
    };
    if let Err(evidence) = validate_group_policy(
        &expected,
        organization,
        repository_trust.repository_id,
        &group_policy,
    ) {
        return Ok(evidence);
    }

    let Some(route) = read_existing_route(
        transport,
        organization_admin,
        organization,
        expected.actions_runner_group_name,
        expected.scale_set_name,
        controller_token,
        intents,
    )
    .await?
    else {
        return Ok(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::PoolBindingMismatch,
        ));
    };

    Ok(verify_organization_pool_policy(
        &expected,
        &repository_trust,
        &group_policy,
        &route,
        SystemTime::now(),
    ))
}

fn validate_preflight_input(
    expected: &PoolBindingView<'_>,
    organization: &str,
    actions_read_token: &str,
    controller_token: &str,
) -> Option<PoolAdmissionEvidence> {
    if expected.allowed_group_workflows.is_empty() {
        return Some(PoolAdmissionEvidence::Unknown(
            PolicyGap::WorkflowRuleSetEmpty,
        ));
    }
    if !valid_preflight_binding(expected, organization, actions_read_token, controller_token) {
        return Some(PoolAdmissionEvidence::Rejected(
            PolicyMismatch::InvalidPolicy,
        ));
    }
    let Some(image) = expected.runner_image else {
        return Some(PoolAdmissionEvidence::Unknown(PolicyGap::MissingField));
    };
    if !super::image::has_required_admission_profile(&image, expected.scale_set_name) {
        return Some(PoolAdmissionEvidence::Unknown(
            PolicyGap::RequiredRunnerProfileUnavailable,
        ));
    }
    None
}

async fn read_existing_route<T, I>(
    transport: &mut T,
    organization_admin: crate::OrganizationAdminEvidence,
    organization: &str,
    group_name: &str,
    scale_set_name: &str,
    controller_token: &str,
    intents: &mut I,
) -> Result<Option<crate::ActionsServiceScaleSetRoute>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
    I: AsyncScopedDiscoveryIntentStore + ?Sized,
{
    let token = issue_organization_discovery_token_async(
        transport,
        organization_admin,
        controller_token,
        intents,
    )
    .await?;
    let admin = exchange_organization_discovery_admin_once_async(transport, token, intents).await?;
    match admin
        .read_scale_set_route_async(transport, group_name, scale_set_name)
        .await?
    {
        ActionsServiceRouteLookup::Found(route)
            if route.organization().eq_ignore_ascii_case(organization) =>
        {
            Ok(Some(route))
        }
        ActionsServiceRouteLookup::Found(_)
        | ActionsServiceRouteLookup::GroupNotFound
        | ActionsServiceRouteLookup::ScaleSetNotFound => Ok(None),
    }
}

fn valid_preflight_binding(
    expected: &PoolBindingView<'_>,
    organization: &str,
    actions_read_token: &str,
    controller_token: &str,
) -> bool {
    valid_expected(expected, organization)
        && crate::registration::is_supported_product_selector(expected.scale_set_name)
        && !actions_read_token.is_empty()
        && !controller_token.is_empty()
}

fn repository_parts(full_name: &str) -> Option<(&str, &str)> {
    let (owner, repository) = full_name.split_once('/')?;
    if owner.is_empty()
        || repository.is_empty()
        || repository.contains('/')
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || !repository
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return None;
    }
    Some((owner, repository))
}
