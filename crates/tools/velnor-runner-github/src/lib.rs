//! Scale Set wire client. DTOs stay separate from core state.
//! Session calls live in [`session`].

mod acquire;
mod actions;
mod error;
mod paths;
pub mod policy;
mod poll;
mod refresh;
mod registration;
mod runner;
mod runner_group_policy;
mod secret;

pub mod session;

pub use acquire::{AcquireOutcome, Certainty, TransportFail, classify_acquire, effect_certainty};
pub use actions::{
    ActionsJob, ActionsJobReconciliation, ActionsJobReconciliationReason,
    ActionsJobReconciliationState, ActionsRepository, ActionsWorkflowAttemptEvidenceGap,
    ActionsWorkflowAttemptJobEvidence, ActionsWorkflowAttemptProviderEvidence,
    ActionsWorkflowAttemptProviderRead, ActionsWorkflowRun, ActionsWorkflowRunArtifactEvidence,
    ForkPullRequestWorkflowSetting, ObservedScaleSetJob, PrivateRepoForkWorkflowSettings,
    get_actions_job, get_actions_repository, get_actions_repository_async,
    get_actions_workflow_run, get_private_repo_fork_workflow_settings,
    get_private_repo_fork_workflow_settings_async,
    read_actions_workflow_attempt_provider_evidence_async, reconcile_observed_scale_set_job,
    reconcile_observed_scale_set_job_async,
};
pub use error::WireError;
pub use paths::{
    CAPACITY_HEADER, acquire_path, capacity_header_value, jit_path, last_message_query,
    scale_set_path,
};
pub use poll::{InnerJob, InnerKind, ParsedBatch, Poll, Statistics, may_ack, parse_poll};
pub use refresh::{RefreshGate, StatusClass, classify_status};
pub use registration::{
    AcquireUnresolvedReason, ActionsServiceRouteLookup, ActionsServiceScaleSetRoute,
    AdminConnection, AdminConnectionCall, AsyncDiscoveryIntentStore, AsyncDiscoveryTransport,
    AsyncScopedDiscoveryIntentStore, CreateLabel, DiscoveryCredentialOutcome,
    DiscoveryCredentialStep, DiscoveryExchange, DiscoveryIntentId, DiscoveryIntentStore,
    DiscoveryStoreFuture, DiscoveryTransport, Label, OrganizationAdminEvidence,
    OrganizationDiscoveryAdmin, OrganizationDiscoveryToken, PoolSessionCapabilityError,
    PopulationObservationSource, RegistrationScope, RegistrationToken, RegistrationTokenCall,
    RepositoryAdminEvidence, RepositoryDiscoveryAdmin, RepositoryDiscoveryToken,
    RepositorySessionCleanupBinding, RepositorySessionCleanupExpectation,
    RepositorySessionCleanupOutcome, RepositorySessionCleanupRoute, RepositorySessionCloseClaim,
    RunnerGroup, ScaleSetById, ScaleSetByName, ScaleSetCreate, ScaleSetFound, ScaleSetView,
    SessionCloseOutcome, SessionPopulationObservation, VerifiedAcquireOutcome, VerifiedAcquiredJob,
    VerifiedAssignedDemand, VerifiedPoolSessionAdmin, VerifiedQueueSession, accept_scale_set,
    accept_scale_set_for, admin_connection, admin_connection_once, admin_token_is_fresh,
    create_body, create_runner_scale_set, enterprise_registration_token_path,
    exchange_organization_discovery_admin_once_async, exchange_repository_discovery_admin_once,
    exchange_repository_discovery_admin_once_async, get_runner_by_name, get_runner_scale_set,
    get_runner_scale_set_by_id, http_create_body, http_create_body_for,
    issue_organization_discovery_token_async, issue_repository_discovery_token,
    issue_repository_discovery_token_async, list_runner_groups, organization_admin_evidence,
    organization_registration_token_path, product_create_labels, product_create_labels_for,
    read_repository_admin_evidence, read_repository_admin_evidence_async, registration_token,
    remove_runner, repository_registration_token_path,
};
pub use runner::RunnerReference;
pub use runner_group_policy::{
    ActionsRunnerGroupPolicy, OrganizationRunnerGroupPolicyEvidence, RunnerGroupAccess,
    RunnerGroupPolicySnapshot, RunnerGroupScope, SelectedOrganization, SelectedRepository,
    find_enterprise_runner_group_policy, find_organization_runner_group_policy,
    find_organization_runner_group_policy_async, get_enterprise_runner_group_policy,
    get_organization_runner_group_policy, read_organization_runner_group_policy_evidence,
    read_organization_runner_group_policy_evidence_async,
};
pub use secret::EncodedJit;
pub use session::{
    Ack, AckScope, BearerRole, Exchange, MessageQueueRoute, Method, QueueSession, RequestPurpose,
    SessionError, SessionRequest, Transport, ack, acquire, create_session, delete_session, jit,
    jit_request, poll, poll_with_trust, refresh_if_current, refresh_queue_request, refresh_session,
    reopen_session,
};
