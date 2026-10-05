//! Scale Set wire client. DTOs stay separate from core state.

mod acquire;
mod error;
mod http;
mod message;
mod offers;
mod paths;
mod poll;
mod provision;
mod refresh;
mod registration;
mod retry;
mod secret;
mod sessions;

pub use acquire::{AcquireOutcome, Certainty, TransportFail, classify_acquire, effect_certainty};
pub use error::{DriverError, WireError};
pub use http::{BuiltRequest, Exchange, QueueAuth, Transport};
pub use message::{Ack, AckStatus, PollIn, acknowledge, poll_queue};
pub use offers::{AcquireIn, acquire_jobs};
pub use paths::{
    CAPACITY_HEADER, acquire_path, capacity_header_value, jit_path, last_message_query,
    scale_set_path,
};
pub use poll::{InnerJob, InnerKind, ParsedBatch, Poll, Statistics, may_ack, parse_poll};
pub use provision::{JitIn, generate_jit, jit_request};
pub use refresh::{RefreshGate, StatusClass, classify_status};
pub use registration::{Label, ScaleSetView, accept_scale_set, create_body};
pub use secret::EncodedJit;
pub use sessions::{
    OpenIn, Opened, Refreshed, Session, SessionIn, delete_session, open_session, refresh_session,
};
