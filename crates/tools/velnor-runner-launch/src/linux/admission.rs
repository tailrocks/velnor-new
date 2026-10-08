//! Source-backed pool preflight, before the fail-closed Linux offer gate.

use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::{Instant as TokioInstant, sleep, timeout_at};

pub use velnor_runner_github::policy::{
    PolicyGap, PolicyMismatch, PoolAdmissionEvidence, VerifiedPoolPolicy,
};
use velnor_runner_github::policy::{
    PoolBindingView, PoolRegistrationScopeView, RunnerImageIdentityView,
    preflight_organization_pool_admission_async,
};
use velnor_runner_host::RegistrationScope;
use velnor_runner_host::{BoundedDiscoveryTransport, ValidatedHostConfigSnapshot};
use velnor_runner_journal::journal::Journal;

use crate::discovery_intents::JournalDiscoveryIntentStore;

use super::{
    LinuxAdmissionState, LinuxLaunchContext, LinuxLaunchCredentials, SIGNAL_POLL, deadline_after,
};

const PREFLIGHT_BUDGET: Duration = Duration::from_secs(120);
pub(super) async fn prepare_admission(
    context: &LinuxLaunchContext,
    credentials: Option<&LinuxLaunchCredentials>,
    journal: &Journal,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
) -> LinuxAdmissionState {
    let Some(profile) = context.snapshot.runner_image_profile() else {
        return LinuxAdmissionState::RunnerProfileUnavailable;
    };
    if velnor_runner_host::verify_runner_profile_admission().is_err() {
        return LinuxAdmissionState::RunnerProfileAdmissionUnavailable;
    }
    let Some(credentials) = credentials else {
        return LinuxAdmissionState::CredentialsUnavailable;
    };
    let Some(expected) = pool_binding_view(&context.snapshot, &profile) else {
        return LinuxAdmissionState::PoolUnknown(PolicyGap::MissingField);
    };

    let preflight = async {
        let mut transport = BoundedDiscoveryTransport::new();
        let mut intents = JournalDiscoveryIntentStore::new(journal);
        preflight_organization_pool_admission_async(
            &mut transport,
            expected,
            &credentials.actions_read,
            &credentials.controller,
            &mut intents,
        )
        .await
    };
    tokio::pin!(preflight);
    let now = Instant::now();
    let Some(preflight_deadline) = now.checked_add(PREFLIGHT_BUDGET) else {
        return LinuxAdmissionState::PoolPreflightUnavailable;
    };
    let mut wake = Box::pin(sleep(SIGNAL_POLL));
    loop {
        tokio::select! {
            result = timeout_at(TokioInstant::from_std(preflight_deadline), &mut preflight) => {
                return match result {
                    Ok(Ok(PoolAdmissionEvidence::Verified(_))) => LinuxAdmissionState::VerifiedOfferGateClosed,
                    Ok(Ok(PoolAdmissionEvidence::Unknown(gap))) => LinuxAdmissionState::PoolUnknown(gap),
                    Ok(Ok(PoolAdmissionEvidence::Rejected(mismatch))) => LinuxAdmissionState::PoolRejected(mismatch),
                    Ok(Err(_)) | Err(_) => LinuxAdmissionState::PoolPreflightUnavailable,
                };
            }
            changed = shutdown.changed() => {
                let latest = *shutdown.borrow_and_update();
                if let Some(value) = latest {
                    *cutoff = Some(value);
                    return LinuxAdmissionState::ShutdownBeforePreflight;
                }
                if changed.is_err() {
                    *cutoff = Some(deadline_after(Instant::now(), context.drain_timeout));
                    return LinuxAdmissionState::ShutdownBeforePreflight;
                }
            }
            () = &mut wake => {
                match journal.draining().await {
                    Ok(true) | Err(_) => {
                        // A drain observed after startup gets one bounded local interval.
                        // Read failure aborts new work and attempts the durable fence.
                        *cutoff = Some(deadline_after(Instant::now(), context.drain_timeout));
                        return LinuxAdmissionState::ShutdownBeforePreflight;
                    }
                    Ok(false) => {}
                }
                wake.as_mut().reset(tokio::time::Instant::now() + SIGNAL_POLL);
            }
        }
    }
}

fn pool_binding_view<'a>(
    snapshot: &'a ValidatedHostConfigSnapshot,
    profile: &velnor_runner_host::RunnerImageProfile,
) -> Option<PoolBindingView<'a>> {
    let config = snapshot.config();
    let binding = snapshot.scale_set_binding();
    let trust = config.trust.as_ref()?;
    let registration_scope = match &binding.scope {
        RegistrationScope::Repository { owner, repository } => {
            PoolRegistrationScopeView::Repository { owner, repository }
        }
        RegistrationScope::Organization { organization } => {
            PoolRegistrationScopeView::Organization { organization }
        }
    };
    Some(PoolBindingView {
        registration_scope,
        target_repository_id: None,
        target_repository_full_name: &config.github.repository,
        scale_set_id: None,
        scale_set_name: &binding.scale_set_name,
        actions_runner_group_id: binding.runner_group_id,
        actions_runner_group_name: &binding.runner_group_name,
        rest_runner_group_id: None,
        runner_image: Some(image_identity(profile)),
        allowed_group_workflows: &trust.allowed_group_workflows,
        policy_digest: snapshot.policy_digest(),
    })
}

fn image_identity(
    profile: &velnor_runner_host::RunnerImageProfile,
) -> RunnerImageIdentityView<'static> {
    RunnerImageIdentityView {
        profile: profile.key(),
        scale_set_name: profile.scale_set_name(),
        platform: profile.platform(),
        runner_image: profile.runner_image(),
        runner_manifest_digest: profile.runner_manifest_digest(),
        runner_index_digest: profile.runner_index_digest(),
        runner_config_digest: profile.runner_config_digest(),
        runner_os: profile.runner_os(),
        runner_release_version: profile.runner_version(),
        runner_release_published_at: profile.runner_release_published_at(),
        runner_requalify_by: profile.runner_requalify_by(),
        dind_image: profile.dind_image(),
        dind_manifest_digest: profile.dind_manifest_digest(),
        dind_index_digest: profile.dind_index_digest(),
        dind_config_digest: profile.dind_config_digest(),
        dind_version: profile.dind_version(),
        dind_source: profile.dind_source(),
        dind_entrypoint_sha256: profile.dind_entrypoint_sha256(),
    }
}
