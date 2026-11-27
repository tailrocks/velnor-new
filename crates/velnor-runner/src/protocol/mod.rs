// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub mod broker;
pub mod error;
pub mod feed;
pub mod http;
pub mod jit;
pub mod oauth;
pub mod results;
mod results_upload;
mod results_wire;
pub mod run_service;
pub mod session;
pub mod timeline;
mod token_backoff;

pub use broker::{Acknowledgement, BrokerClient, BrokerPoll};
pub use error::{
    BrokerErrorCategory, GitHubApiError, ServiceErrorKind, broker_error_category,
    service_error_kind,
};
pub use feed::{FeedStreamClient, FeedWebSocket};
pub use http::{
    GithubHttpResponse, RUNNER_USER_AGENT, RunnerOs, github_json_http_response,
    github_json_request,
};
pub use jit::{
    DecodedJitConfig, DecodedJitCredentials, DecodedJitRunnerSettings, SessionRecovery,
    decode_jit_config,
};
pub use oauth::{
    OAuthAccessToken, OAuthClient, OAuthErrorKind, OAuthJwtCredentials, OAuthTokenError,
    oauth_error_kind,
};
pub use results::{BLOCK_BUDGET, StepConclusion, StepStatus, TwirpResultsClient, TwirpStep};
pub use run_service::{
    AcquireJobOutcome, AcquireJobSkipReason, AcquireRetriesExhausted, AcquiredJobPayload,
    CompletionAcknowledgement, RenewJobResponse, RetryDelays, RunServiceAnnotation, RunServiceAnnotationLevel,
    RunServiceClient, RunServiceCompleteJob, RunServiceStepResult, RunServiceTelemetry,
    RunServiceVariableValue, acquire_job, is_transient_acquire_error, renew_failure_is_job_gone,
};
pub use session::{
    RUNNER_JOB_REQUEST, RunnerJobRequestRef, RunnerStatus, TaskAgentMessage, TaskAgentReference,
    TaskAgentSession,
};
pub use timeline::{TaskResult, TimelineRecordState};
pub use token_backoff::TokenBackoff;
