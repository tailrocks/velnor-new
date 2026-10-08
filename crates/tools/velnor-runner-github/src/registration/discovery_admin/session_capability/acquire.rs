//! Refresh routing for Acquire, whose URL uses the Actions Service origin while
//! its bearer is the session queue token.

use crate::DiscoveryTransport;
use crate::session::{SessionError, SessionRequest, refresh_queue_request};

use super::VerifiedPoolSessionAdmin;
use super::origin::bind_admin_origin;
use super::types::VerifiedQueueSession;

pub(super) fn refresh_acquire_and_route<T>(
    transport: &mut T,
    capability: &VerifiedPoolSessionAdmin,
    session: &mut VerifiedQueueSession,
    request: &mut SessionRequest,
) -> Result<(), SessionError>
where
    T: DiscoveryTransport + ?Sized,
{
    bind_admin_origin(transport, capability)?;
    let queue_url = refresh_queue_request(
        transport,
        capability.binding.scale_set_id,
        &mut session.inner,
        capability.connection.expose_token(),
        request,
    )?;
    let queue_route = transport.bind_message_queue_origin(queue_url)?;
    session.queue_route.replace_with(queue_route);
    // Acquire remains on the Actions Service URL with the refreshed session
    // bearer; the new MessageQueueURL is cached only for queue operations.
    bind_admin_origin(transport, capability)?;
    Ok(())
}
