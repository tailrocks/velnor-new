//! Run readiness refreshes in a separate thread with its own Tokio runtime.

use std::{
    future::Future,
    path::PathBuf,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use bollard::Docker;
use tokio::sync::oneshot;

use crate::{
    controller_ipc::ControlState,
    error::HostError,
    journal::Journal,
    launch::{ReleaseReadSecret, SelectedEngineContext},
};

use super::refresh;

const REFRESH_PAUSE: Duration = Duration::from_secs(2);

pub(in crate::host_controller::loop_driver::controller) struct ReadinessWorker {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl ReadinessWorker {
    pub(in crate::host_controller::loop_driver::controller) fn start(
        state: PathBuf,
        docker: Docker,
        journal: Journal,
        selected: SelectedEngineContext,
        control: ControlState,
        secret: Arc<ReleaseReadSecret>,
    ) -> Result<Self, HostError> {
        spawn_refresh_worker(move || {
            let state = state.clone();
            let docker = docker.clone();
            let journal = journal.clone();
            let selected = selected.clone();
            let control = control.clone();
            let secret = Arc::clone(&secret);
            async move {
                if let Err(error) =
                    refresh(&state, &docker, &journal, &selected, &control, &secret).await
                {
                    eprintln!(
                        "The controller could not refresh readiness evidence. Error: {error}."
                    );
                    if let Err(clear_error) = control.clear_readiness() {
                        eprintln!(
                            "The controller could not clear readiness evidence. Error: {clear_error}."
                        );
                    }
                }
                Ok(())
            }
        })
    }
}

pub(super) fn spawn_refresh_worker<F, Fut>(refresh: F) -> Result<ReadinessWorker, HostError>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), HostError>> + Send + 'static,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    let (stop, stopped) = oneshot::channel();
    let thread = thread::Builder::new()
        .name("velnor-readiness".to_owned())
        .spawn(move || {
            runtime.block_on(async move {
                let mut stopped = stopped;
                loop {
                    if let Err(error) = refresh().await {
                        eprintln!("The readiness refresh thread failed. Error: {error}.");
                    }
                    tokio::select! {
                        _ = &mut stopped => break,
                        () = tokio::time::sleep(REFRESH_PAUSE) => {}
                    }
                }
            });
        })
        .map_err(|_| HostError::Journal)?;
    Ok(ReadinessWorker {
        stop: Some(stop),
        thread: Some(thread),
    })
}

impl Drop for ReadinessWorker {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _joined = thread.join();
        }
    }
}
