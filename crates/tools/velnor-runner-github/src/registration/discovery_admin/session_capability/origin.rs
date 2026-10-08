//! Bind the shared Actions transport to the exact origin for each operation.

use crate::{DiscoveryTransport, SessionError};

use super::VerifiedPoolSessionAdmin;
use super::types::VerifiedQueueSession;

pub(super) fn bind_admin_origin<T>(
    transport: &mut T,
    capability: &VerifiedPoolSessionAdmin,
) -> Result<(), SessionError>
where
    T: DiscoveryTransport + ?Sized,
{
    bind_service_origin(transport, capability.connection.expose_url())
}

pub(super) fn bind_queue_origin<T>(
    transport: &mut T,
    session: &VerifiedQueueSession,
) -> Result<(), SessionError>
where
    T: DiscoveryTransport + ?Sized,
{
    bind_service_origin(transport, &session.inner.message_queue_url)
}

fn bind_service_origin<T>(transport: &mut T, url: &str) -> Result<(), SessionError>
where
    T: DiscoveryTransport + ?Sized,
{
    transport.bind_actions_service_origin(url)
}
