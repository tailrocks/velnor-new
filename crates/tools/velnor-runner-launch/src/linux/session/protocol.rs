use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::time::Instant;

use tokio::sync::watch;
use velnor_runner_github::{
    DiscoveryExchange, Exchange, SessionError, SessionRequest, TransportFail,
    VerifiedAssignedDemand, VerifiedPoolSessionAdmin, VerifiedQueueSession,
};
use velnor_runner_host::{BoundedDiscoveryTransport, HostError};

use super::cutoff::DispatchFence;
use super::deadline_transport::{
    AsyncDeadlineTransport, CancelDispatchOnDrop, DeadlineBoundTransport, SyncDeadlineTransport,
};

impl SyncDeadlineTransport for BoundedDiscoveryTransport {
    fn exchange_until(
        &mut self,
        request: &SessionRequest,
        absolute_cutoff: Option<Instant>,
        cancellation: &AtomicBool,
    ) -> Result<Exchange, TransportFail> {
        BoundedDiscoveryTransport::exchange_until(self, request, absolute_cutoff, cancellation)
    }
}

impl AsyncDeadlineTransport for BoundedDiscoveryTransport {
    fn exchange_discovery_until(
        &mut self,
        request: SessionRequest,
        absolute_cutoff: Option<Instant>,
        cancellation: Arc<AtomicBool>,
    ) -> DiscoveryExchange {
        BoundedDiscoveryTransport::exchange_discovery_until(
            self,
            request,
            absolute_cutoff,
            cancellation,
        )
    }
}

pub(in crate::linux::session) struct Protocol {
    pub(in crate::linux::session) admin: VerifiedPoolSessionAdmin,
    pub(in crate::linux::session) session: VerifiedQueueSession,
    pub(in crate::linux::session) assigned_demand: Option<VerifiedAssignedDemand>,
}

pub(in crate::linux::session) async fn protocol_call<R, F>(
    protocol: Arc<Mutex<Protocol>>,
    dispatch: DispatchFence,
    shutdown: watch::Receiver<Option<Instant>>,
    phase_deadline: Option<Instant>,
    call: F,
) -> Result<R, HostError>
where
    R: Send + 'static,
    F: FnOnce(
            &mut Protocol,
            &mut DeadlineBoundTransport<BoundedDiscoveryTransport>,
        ) -> Result<R, SessionError>
        + Send
        + 'static,
{
    let _cancel_if_dropped = CancelDispatchOnDrop(dispatch.clone());
    tokio::task::spawn_blocking(move || {
        if !dispatch.allowed(&shutdown, phase_deadline) {
            return Err(HostError::Journal);
        }
        let mut protocol = protocol.lock().map_err(|_| HostError::Journal)?;
        if !dispatch.begin(&shutdown, phase_deadline) {
            return Err(HostError::Journal);
        }
        let mut transport = DeadlineBoundTransport::new(
            BoundedDiscoveryTransport::new(),
            dispatch.clone(),
            shutdown.clone(),
            phase_deadline,
        );
        call(&mut protocol, &mut transport).map_err(|_| HostError::Journal)
    })
    .await
    .map_err(|_| HostError::Journal)?
}

pub(in crate::linux::session) async fn protocol_read<R, F>(
    protocol: Arc<Mutex<Protocol>>,
    read: F,
) -> Result<R, HostError>
where
    R: Send + 'static,
    F: FnOnce(&Protocol) -> R + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let protocol = protocol.lock().map_err(|_| HostError::Journal)?;
        Ok(read(&protocol))
    })
    .await
    .map_err(|_| HostError::Journal)?
}
