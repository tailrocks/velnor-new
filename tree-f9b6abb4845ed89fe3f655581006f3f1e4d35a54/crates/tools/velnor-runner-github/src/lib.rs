//! Scale Set wire client. DTOs stay separate from core state.
//! Session calls live in [`session`].

mod acquire;
mod actions;
mod error;
mod paths;
mod poll;
mod refresh;
mod registration;
mod runner;
mod secret;

pub mod session;

pub use acquire::{AcquireOutcome, Certainty, TransportFail, classify_acquire, effect_certainty};
pub use actions::{
    ActionsJob, ActionsRepository, ActionsWorkflowRun, ForkPullRequestWorkflowSetting,
    PrivateRepoForkWorkflowSettings, get_actions_job, get_actions_repository,
    get_actions_workflow_run, get_private_repo_fork_workflow_settings,
};
pub use error::WireError;
pub use paths::{
    CAPACITY_HEADER, acquire_path, capacity_header_value, jit_path, last_message_query,
    scale_set_path,
};
pub use poll::{InnerJob, InnerKind, ParsedBatch, Poll, Statistics, may_ack, parse_poll};
pub use refresh::{RefreshGate, StatusClass, classify_status};
pub use registration::{
    AdminConnection, AdminConnectionCall, CreateLabel, Label, RegistrationScope, RegistrationToken,
    RegistrationTokenCall, RunnerGroup, ScaleSetById, ScaleSetByName, ScaleSetCreate,
    ScaleSetFound, ScaleSetView, accept_scale_set, admin_connection, admin_token_is_fresh,
    create_body, create_runner_scale_set, enterprise_registration_token_path, get_runner_by_name,
    get_runner_scale_set, get_runner_scale_set_by_id, http_create_body, list_runner_groups,
    organization_registration_token_path, product_create_labels, registration_token, remove_runner,
    repository_registration_token_path,
};
pub use runner::RunnerReference;
pub use secret::EncodedJit;
pub use session::{
    Ack, AckScope, Exchange, Method, QueueSession, SessionError, SessionRequest, Transport, ack,
    acquire, create_session, delete_session, jit, jit_request, poll, refresh_if_current,
    refresh_queue_request, refresh_session, reopen_session,
};
