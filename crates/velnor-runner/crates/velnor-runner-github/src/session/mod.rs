//! Session driver. Callers inject [`Transport`]. This module does not open a socket.

mod acknowledge;
mod close;
mod config;
mod error;
mod jobs;
mod messages;
mod request;
mod retry;

pub use acknowledge::{Ack, ack};
pub use close::delete_session;
pub use config::jit;
pub use error::SessionError;
pub use jobs::acquire;
pub use messages::poll;
pub use request::{Exchange, Method, SessionRequest, Transport};
