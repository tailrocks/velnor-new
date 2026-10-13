//! Source-backed pool preflight, before the fail-closed Linux offer gate.

use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::{Instant as TokioInstant, sleep, timeout_at};

use velnor_runner_github::VerifiedPoolSessionAdmin;
pub use velnor_runner_github::policy::{
    PolicyGap, PolicyMismatch, PoolAdmissionEvidence, VerifiedPoolPolicy,
};
use velnor_runner_github::policy::{
    PoolBindingView, PoolRegistrationScopeView, RunnerImageIdentityView,
    preflight_pool_admission_with_admin_async,
};
use velnor_runner_host::RegistrationScope;
use velnor_runner_host::{BoundedDiscoveryTransport, ValidatedHostConfigSnapshot};
use velnor_runner_journal::journal::Journal;

use crate::discovery_intents::JournalDiscoveryIntentStore;

use super::session::observe_cutoff_before_deadline;
use super::session::{CancelDispatchOnDrop, DeadlineBoundTransport, DispatchFence};
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
) -> AdmissionPreparation {
    let Some(credentials) = credentials else {
        return AdmissionPreparation::blocked(LinuxAdmissionState::CredentialsUnavailable);
    };
    let expected = match preflight_binding(context) {
        Ok(expected) => expected,
        Err(state) => return AdmissionPreparation::blocked(state),
    };
    let preflight_deadline = deadline_after(Instant::now(), PREFLIGHT_BUDGET);
    match observe_cutoff_before_deadline(
        journal,
        shutdown,
        cutoff,
        context.drain_timeout(),
        Some(preflight_deadline),
    )
    .await
    {
        Some(false) => {}
        Some(true) => {
            return AdmissionPreparation::blocked(LinuxAdmissionState::ShutdownBeforePreflight);
        }
        None => {
            return AdmissionPreparation::blocked(LinuxAdmissionState::PoolPreflightUnavailable);
        }
    }
    match wait_preflight(
        context,
        credentials,
        journal,
        shutdown,
        cutoff,
        preflight_deadline,
        &expected,
    )
    .await
    {
        Some(preflight) => admission_result(&preflight.evidence, preflight.verified_admin),
        None => AdmissionPreparation::blocked(LinuxAdmissionState::PoolPreflightUnavailable),
    }
}

fn preflight_binding(
    context: &LinuxLaunchContext,
) -> Result<PoolBindingView<'_>, LinuxAdmissionState> {
    let Some(profile) = context.snapshot.runner_image_profile() else {
        return Err(LinuxAdmissionState::RunnerProfileUnavailable);
    };
    if velnor_runner_host::verify_runner_profile_admission().is_err() {
        return Err(LinuxAdmissionState::RunnerProfileAdmissionUnavailable);
    }
    if context
        .snapshot
        .with_job_trust_policy_view(|_| ())
        .is_none()
    {
        return Err(LinuxAdmissionState::PoolUnknown(PolicyGap::MissingField));
    }
    pool_binding_view(&context.snapshot, &profile)
        .ok_or(LinuxAdmissionState::PoolUnknown(PolicyGap::MissingField))
}

async fn wait_preflight(
    context: &LinuxLaunchContext,
    credentials: &LinuxLaunchCredentials,
    journal: &Journal,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
    preflight_deadline: Instant,
    expected: &PoolBindingView<'_>,
) -> Option<velnor_runner_github::policy::PoolAdmissionPreflight> {
    let dispatch = DispatchFence::new();
    let preflight_dispatch = dispatch.clone();
    let preflight_shutdown = shutdown.clone();
    let preflight = async {
        let _cancel = CancelDispatchOnDrop(preflight_dispatch.clone());
        let mut transport = DeadlineBoundTransport::new(
            BoundedDiscoveryTransport::new(),
            preflight_dispatch.clone(),
            preflight_shutdown.clone(),
            Some(preflight_deadline),
        );
        let mut intents = JournalDiscoveryIntentStore::new(journal);
        preflight_pool_admission_with_admin_async(
            &mut transport,
            *expected,
            &credentials.actions_read,
            &credentials.controller,
            &mut intents,
        )
        .await
    };
    tokio::pin!(preflight);
    let mut wake = Box::pin(sleep(SIGNAL_POLL));
    loop {
        tokio::select! {
            result = timeout_at(TokioInstant::from_std(preflight_deadline), &mut preflight) => {
                return match result { Ok(Ok(value)) => Some(value), Ok(Err(_)) | Err(_) => None };
            }
            changed = shutdown.changed() => {
                let latest = *shutdown.borrow_and_update();
                if let Some(value) = latest {
                    *cutoff = Some(value);
                    let _ = observe_cutoff_before_deadline(
                        journal,
                        shutdown,
                        cutoff,
                        context.drain_timeout(),
                        Some(preflight_deadline),
                    ).await;
                    return None;
                }
                if changed.is_err() {
                    *cutoff = Some(deadline_after(Instant::now(), context.drain_timeout));
                    let _ = observe_cutoff_before_deadline(
                        journal,
                        shutdown,
                        cutoff,
                        context.drain_timeout(),
                        Some(preflight_deadline),
                    ).await;
                    return None;
                }
            }
            () = &mut wake => {
                if !matches!(observe_cutoff_before_deadline(
                    journal,
                    shutdown,
                    cutoff,
                    context.drain_timeout(),
                    Some(preflight_deadline),
                ).await, Some(false)) || cutoff.is_some() {
                    return None;
                }
                wake.as_mut().reset(tokio::time::Instant::now() + SIGNAL_POLL);
            }
        }
    }
}

pub(super) struct AdmissionPreparation {
    pub(super) state: LinuxAdmissionState,
    pub(super) verified_admin: Option<VerifiedPoolSessionAdmin>,
}

impl AdmissionPreparation {
    fn blocked(state: LinuxAdmissionState) -> Self {
        Self {
            state,
            verified_admin: None,
        }
    }
}

fn admission_result(
    evidence: &PoolAdmissionEvidence,
    verified_admin: Option<VerifiedPoolSessionAdmin>,
) -> AdmissionPreparation {
    match evidence {
        PoolAdmissionEvidence::Verified(_) => match verified_admin {
            Some(admin) if repository_session_supported(admin.binding()) => AdmissionPreparation {
                state: LinuxAdmissionState::VerifiedSessionReady,
                verified_admin: Some(admin),
            },
            Some(_) => {
                AdmissionPreparation::blocked(LinuxAdmissionState::SessionCloseUnsupportedScope)
            }
            None => AdmissionPreparation::blocked(LinuxAdmissionState::PoolPreflightUnavailable),
        },
        PoolAdmissionEvidence::Unknown(gap) => {
            AdmissionPreparation::blocked(LinuxAdmissionState::PoolUnknown(*gap))
        }
        PoolAdmissionEvidence::Rejected(mismatch) => {
            AdmissionPreparation::blocked(LinuxAdmissionState::PoolRejected(*mismatch))
        }
    }
}

fn repository_session_supported(binding: &velnor_runner_github::policy::PoolBinding) -> bool {
    matches!(
        binding.registration_scope,
        velnor_runner_github::policy::PoolRegistrationScope::Repository { .. }
    )
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
