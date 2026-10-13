//! Session driver. Callers inject [`Transport`]. This module does not open a socket.

mod acknowledge;
mod close;
mod config;
mod error;
mod jobs;
mod messages;
mod open;
mod reopen;
mod request;
mod retry;
mod route;

pub(crate) use acknowledge::ack_with_route;
pub(crate) use close::{delete_session_async, safe_path_segment};
pub(crate) use error::reject;
pub(crate) use messages::poll_with_trust_route;
pub(crate) use retry::{API_QUERY, bearer, execute, fail_exchange, json_content, user_agent};

pub use acknowledge::{Ack, AckScope, ack};
pub use close::delete_session;
pub use config::{jit, jit_request};
pub use error::SessionError;
pub use jobs::acquire;
pub use messages::{poll, poll_with_trust};
pub use open::{
    QueueSession, create_session, refresh_if_current, refresh_queue_request, refresh_session,
};
pub use reopen::reopen_session;
pub use request::{BearerRole, Exchange, Method, RequestPurpose, SessionRequest, Transport};

pub use route::MessageQueueRoute;
