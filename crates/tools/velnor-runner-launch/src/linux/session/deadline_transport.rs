//! Recheck the retained phase cutoff at every wire exchange.

use std::sync::{Arc, atomic::AtomicBool};
use std::time::Instant;

use tokio::sync::watch;
use velnor_runner_github::{
    AsyncDiscoveryTransport, DiscoveryExchange, DiscoveryTransport, Exchange, MessageQueueRoute,
    SessionError, SessionRequest, Transport, TransportFail,
};

use super::cutoff::DispatchFence;

/// Host transports must clamp every exchange to this absolute cutoff and
/// observe cancellation while a bounded curl child is running.
pub(in crate::linux) trait SyncDeadlineTransport: DiscoveryTransport {
    fn exchange_until(
        &mut self,
        request: &SessionRequest,
        absolute_cutoff: Option<Instant>,
        cancellation: &AtomicBool,
    ) -> Result<Exchange, TransportFail>;
}

/// Async discovery transport with the same absolute cutoff and cancellation
/// contract as the synchronous Actions-session route.
pub(in crate::linux) trait AsyncDeadlineTransport:
    AsyncDiscoveryTransport
{
    fn exchange_discovery_until(
        &mut self,
        request: SessionRequest,
        absolute_cutoff: Option<Instant>,
        cancellation: Arc<AtomicBool>,
    ) -> DiscoveryExchange;
}

/// Reusable transport facade for one coordinator phase. It retains the same
/// absolute stop cutoff across helper-internal refresh and replay requests.
pub(in crate::linux) struct DeadlineBoundTransport<T> {
    inner: T,
    dispatch: DispatchFence,
    shutdown: watch::Receiver<Option<Instant>>,
    phase_deadline: Option<Instant>,
}

impl<T> DeadlineBoundTransport<T> {
    pub(in crate::linux) fn new(
        inner: T,
        dispatch: DispatchFence,
        shutdown: watch::Receiver<Option<Instant>>,
        phase_deadline: Option<Instant>,
    ) -> Self {
        Self {
            inner,
            dispatch,
            shutdown,
            phase_deadline,
        }
    }

    fn wire_deadline(&self) -> Result<Option<Instant>, TransportFail> {
        self.dispatch
            .wire_deadline(&self.shutdown, self.phase_deadline)
            .map_err(|()| TransportFail::Timeout)
    }
}

impl<T: SyncDeadlineTransport> Transport for DeadlineBoundTransport<T> {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let deadline = self.wire_deadline()?;
        let cancellation = self.dispatch.cancellation_flag();
        self.inner
            .exchange_until(request, deadline, cancellation.as_ref())
    }
}

impl<T: SyncDeadlineTransport> DiscoveryTransport for DeadlineBoundTransport<T> {
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
        self.inner.bind_github_api_origin()
    }

    fn bind_actions_service_origin(&mut self, url: &str) -> Result<(), SessionError> {
        self.inner.bind_actions_service_origin(url)
    }

    fn bind_message_queue_origin(&mut self, url: &str) -> Result<MessageQueueRoute, SessionError> {
        self.inner.bind_message_queue_origin(url)
    }
}

impl<T: AsyncDeadlineTransport> AsyncDiscoveryTransport for DeadlineBoundTransport<T> {
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
        self.inner.bind_github_api_origin()
    }

    fn bind_actions_service_origin(&mut self, url: &str) -> Result<(), SessionError> {
        self.inner.bind_actions_service_origin(url)
    }

    fn exchange_discovery(&mut self, request: SessionRequest) -> DiscoveryExchange {
        let cancellation = self.dispatch.cancellation_flag();
        match self.wire_deadline() {
            Ok(deadline) => self
                .inner
                .exchange_discovery_until(request, deadline, cancellation),
            Err(error) => DiscoveryExchange::new(async move { Err(error) }, cancellation),
        }
    }
}

/// Cancel work that has not reached its first exchange, and stop any later
/// exchange in a helper that already received a response.
pub(in crate::linux) struct CancelDispatchOnDrop(pub(in crate::linux) DispatchFence);

impl Drop for CancelDispatchOnDrop {
    fn drop(&mut self) {
        self.0.cancel_pending();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::{Duration, Instant};

    use tokio::sync::watch;
    use velnor_runner_github::{
        AsyncDiscoveryTransport, BearerRole, DiscoveryExchange, Exchange, MessageQueueRoute,
        Method, RequestPurpose, SessionError, SessionRequest, Transport, TransportFail,
    };

    use super::{AsyncDeadlineTransport, DeadlineBoundTransport, SyncDeadlineTransport};
    use crate::linux::session::cutoff::DispatchFence;

    #[derive(Clone, Default)]
    struct FakeState {
        calls: Arc<AtomicUsize>,
        cutoffs: Arc<Mutex<Vec<Option<Instant>>>>,
    }

    struct FakeTransport(FakeState);

    impl Transport for FakeTransport {
        fn exchange(&mut self, _: &SessionRequest) -> Result<Exchange, TransportFail> {
            Err(TransportFail::Reset)
        }
    }

    impl velnor_runner_github::DiscoveryTransport for FakeTransport {
        fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
            Ok(())
        }

        fn bind_actions_service_origin(&mut self, _: &str) -> Result<(), SessionError> {
            Ok(())
        }

        fn bind_message_queue_origin(
            &mut self,
            _: &str,
        ) -> Result<MessageQueueRoute, SessionError> {
            Err(SessionError::Uncertain)
        }
    }

    impl SyncDeadlineTransport for FakeTransport {
        fn exchange_until(
            &mut self,
            _: &SessionRequest,
            absolute_cutoff: Option<Instant>,
            cancellation: &std::sync::atomic::AtomicBool,
        ) -> Result<Exchange, TransportFail> {
            self.0
                .cutoffs
                .lock()
                .expect("cutoff lock")
                .push(absolute_cutoff);
            if cancellation.load(Ordering::Acquire)
                || absolute_cutoff.is_some_and(|cutoff| Instant::now() >= cutoff)
            {
                return Err(TransportFail::Timeout);
            }
            self.0.calls.fetch_add(1, Ordering::AcqRel);
            Ok(Exchange {
                status: 401,
                body: Vec::new(),
            })
        }
    }

    #[derive(Clone, Default)]
    struct AsyncFakeState {
        calls: Arc<AtomicUsize>,
        cutoffs: Arc<Mutex<Vec<Option<Instant>>>>,
    }

    struct AsyncFakeTransport(AsyncFakeState);

    impl AsyncDiscoveryTransport for AsyncFakeTransport {
        fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
            Ok(())
        }

        fn bind_actions_service_origin(&mut self, _: &str) -> Result<(), SessionError> {
            Ok(())
        }

        fn exchange_discovery(&mut self, request: SessionRequest) -> DiscoveryExchange {
            self.exchange_discovery_until(
                request,
                None,
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
        }
    }

    impl AsyncDeadlineTransport for AsyncFakeTransport {
        fn exchange_discovery_until(
            &mut self,
            _: SessionRequest,
            absolute_cutoff: Option<Instant>,
            cancellation: Arc<std::sync::atomic::AtomicBool>,
        ) -> DiscoveryExchange {
            self.0
                .cutoffs
                .lock()
                .expect("cutoff lock")
                .push(absolute_cutoff);
            if cancellation.load(Ordering::Acquire)
                || absolute_cutoff.is_some_and(|cutoff| Instant::now() >= cutoff)
            {
                return DiscoveryExchange::new(async { Err(TransportFail::Timeout) }, cancellation);
            }
            self.0.calls.fetch_add(1, Ordering::AcqRel);
            DiscoveryExchange::new(
                async {
                    Ok(Exchange {
                        status: 401,
                        body: Vec::new(),
                    })
                },
                cancellation,
            )
        }
    }

    fn request() -> SessionRequest {
        SessionRequest {
            purpose: RequestPurpose::ActionsRead,
            bearer_role: BearerRole::GithubRestCredential,
            method: Method::Get,
            path: "repos/owner/repository/actions/runs/1".to_owned(),
            query: None,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn cutoff_and_cancellation_are_checked_for_each_refresh_or_replay_exchange() {
        let state = FakeState::default();
        let dispatch = DispatchFence::new();
        let (sender, shutdown) = watch::channel(None);
        let cutoff = Instant::now() + Duration::from_secs(10);
        assert!(dispatch.begin(&shutdown, Some(cutoff)));
        let mut transport = DeadlineBoundTransport::new(
            FakeTransport(state.clone()),
            dispatch.clone(),
            shutdown,
            Some(cutoff),
        );

        assert_eq!(
            transport.exchange(&request()).expect("initial 401").status,
            401
        );
        assert_eq!(
            transport.exchange(&request()).expect("refresh 401").status,
            401
        );
        sender
            .send(Some(
                Instant::now()
                    .checked_sub(Duration::from_millis(1))
                    .expect("monotonic clock has a recent past"),
            ))
            .expect("shutdown watcher remains open");
        assert_eq!(transport.exchange(&request()), Err(TransportFail::Timeout));

        assert_eq!(state.calls.load(Ordering::Acquire), 2);
        let observed = state.cutoffs.lock().expect("cutoff lock");
        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0], Some(cutoff));
        assert_eq!(observed[1], Some(cutoff));
    }

    #[test]
    fn a_cancelled_queued_dispatch_never_reaches_the_wire() {
        let state = FakeState::default();
        let dispatch = DispatchFence::new();
        let (_sender, shutdown) = watch::channel(None);
        dispatch.cancel_pending();
        let mut transport =
            DeadlineBoundTransport::new(FakeTransport(state.clone()), dispatch, shutdown, None);

        assert_eq!(transport.exchange(&request()), Err(TransportFail::Timeout));
        assert_eq!(state.calls.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn async_preflight_rechecks_cutoff_for_each_exchange() {
        let state = AsyncFakeState::default();
        let dispatch = DispatchFence::new();
        let (sender, shutdown) = watch::channel(None);
        let cutoff = Instant::now() + Duration::from_secs(10);
        assert!(dispatch.begin(&shutdown, Some(cutoff)));
        let mut transport = DeadlineBoundTransport::new(
            AsyncFakeTransport(state.clone()),
            dispatch,
            shutdown,
            Some(cutoff),
        );

        assert_eq!(
            transport
                .exchange_discovery(request())
                .await
                .expect("first response")
                .status,
            401
        );
        sender
            .send(Some(
                Instant::now()
                    .checked_sub(Duration::from_millis(1))
                    .expect("monotonic clock has a recent past"),
            ))
            .expect("shutdown watcher remains open");
        assert_eq!(
            transport.exchange_discovery(request()).await,
            Err(TransportFail::Timeout)
        );
        assert_eq!(state.calls.load(Ordering::Acquire), 1);
        let observed = state.cutoffs.lock().expect("cutoff lock");
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0], Some(cutoff));
    }
}
