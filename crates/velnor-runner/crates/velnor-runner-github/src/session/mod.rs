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

pub(crate) use error::reject;
pub(crate) use retry::{API_QUERY, bearer, execute, json_content, user_agent};

pub use acknowledge::{Ack, AckScope, ack};
pub use close::delete_session;
pub use config::{jit, jit_request};
pub use error::SessionError;
pub use jobs::acquire;
pub use messages::poll;
pub use open::{QueueSession, create_session, refresh_if_current, refresh_session};
pub use reopen::reopen_session;
pub use request::{Exchange, Method, SessionRequest, Transport};
