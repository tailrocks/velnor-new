//! Scale Set wire client. DTOs stay separate from core state.

mod acquire;
mod error;
mod paths;
mod poll;
mod refresh;
mod registration;
mod secret;

pub use acquire::{AcquireOutcome, Certainty, TransportFail, classify_acquire, effect_certainty};
pub use error::WireError;
pub use paths::{
    CAPACITY_HEADER, acquire_path, capacity_header_value, jit_path, last_message_query,
    scale_set_path,
};
pub use poll::{InnerJob, InnerKind, ParsedBatch, Poll, Statistics, may_ack, parse_poll};
pub use refresh::{RefreshGate, StatusClass, classify_status};
pub use registration::{Label, ScaleSetView, accept_scale_set, create_body};
pub use secret::EncodedJit;
