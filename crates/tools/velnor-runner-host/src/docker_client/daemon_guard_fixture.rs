use std::future::Future;
use std::time::Duration;

use tokio::net::{UnixListener, UnixStream, unix::SocketAddr};
use tokio::task::JoinHandle;

pub(super) const STEP_TIMEOUT: Duration = Duration::from_secs(3);
const SERVER_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) struct FixtureServer<T> {
    handle: Option<JoinHandle<T>>,
}

impl<T: Send + 'static> FixtureServer<T> {
    pub(super) fn spawn(future: impl Future<Output = T> + Send + 'static) -> Self {
        Self {
            handle: Some(tokio::spawn(future)),
        }
    }

    pub(super) async fn finish(mut self) -> Result<T, String> {
        let handle = self
            .handle
            .as_mut()
            .ok_or_else(|| "fixture server already finished".to_owned())?;
        let joined = tokio::time::timeout(SERVER_TIMEOUT, handle)
            .await
            .map_err(|_| "fixture server exceeded its deadline".to_owned())?
            .map_err(|error| format!("fixture server task failed: {error}"))?;
        self.handle.take();
        Ok(joined)
    }
}

impl<T> Drop for FixtureServer<T> {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

pub(super) async fn accept_bounded(
    listener: &UnixListener,
) -> Result<(UnixStream, SocketAddr), String> {
    tokio::time::timeout(STEP_TIMEOUT, listener.accept())
        .await
        .map_err(|_| "fixture accept exceeded its deadline".to_owned())?
        .map_err(|error| error.to_string())
}

pub(super) async fn bounded<T>(
    duration: Duration,
    future: impl Future<Output = Result<T, String>>,
    operation: &str,
) -> Result<T, String> {
    tokio::time::timeout(duration, future)
        .await
        .map_err(|_| format!("fixture {operation} exceeded its deadline"))?
}
