//! Refresh routing for Acquire, whose URL uses the Actions Service origin while
//! its bearer is the session queue token.

use crate::DiscoveryTransport;
use crate::session::{SessionError, SessionRequest, refresh_queue_request};

use super::origin::bind_admin_origin;
use super::types::VerifiedQueueSession;
use super::{VerifiedPoolSessionAdmin, valid_queue_path};

pub(super) fn refresh_acquire_and_route<T, F>(
    transport: &mut T,
    capability: &VerifiedPoolSessionAdmin,
    session: &mut VerifiedQueueSession,
    request: &mut SessionRequest,
    route_after_refresh: &mut F,
) -> Result<(), SessionError>
where
    T: DiscoveryTransport + ?Sized,
    F: FnMut(&mut T, &str, &mut SessionRequest) -> Result<String, SessionError>,
{
    let acquire_path = request.path.clone();
    bind_admin_origin(transport, capability)?;
    let queue_url = refresh_queue_request(
        transport,
        capability.binding.scale_set_id,
        &mut session.inner,
        capability.connection.expose_token(),
        request,
    )?;
    let queue_path = route_after_refresh(transport, queue_url, request)?;
    if !valid_queue_path(&queue_path) {
        return Err(crate::SessionError::Wire(
            crate::WireError::RegistrationRejected,
        ));
    }

    session.queue_path = queue_path;
    request.path = acquire_path;
    // The callback binds the refreshed queue URL for future polling/ACKs.
    // Acquire itself is dispatched against the Actions Service URL, with the
    // refreshed queue token in its Authorization header.
    bind_admin_origin(transport, capability)?;
    Ok(())
}
