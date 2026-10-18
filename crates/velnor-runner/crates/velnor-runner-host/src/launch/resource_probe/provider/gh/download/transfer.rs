use tokio::process::Child;
use tokio::sync::oneshot;

use super::super::super::ProviderFailure;
use super::FirstCompletion;

pub(super) async fn complete_transfer<C>(
    first: FirstCompletion,
    child: &mut Child,
    mut copy: std::pin::Pin<&mut C>,
    cancel: &mut oneshot::Receiver<()>,
    deadline_at: tokio::time::Instant,
    #[cfg(test)] copy_complete: Option<oneshot::Sender<()>>,
) -> Result<(), ProviderFailure>
where
    C: std::future::Future<Output = Result<(), ProviderFailure>>,
{
    match first {
        FirstCompletion::Copy(Ok(())) => {
            #[cfg(test)]
            if let Some(copy_complete) = copy_complete {
                let _send_result = copy_complete.send(());
            }
            tokio::select! {
                biased;
                _ = &mut *cancel => {
                    kill_child_and_wait(child).await;
                    Err(ProviderFailure::GhOutput)
                }
                result = tokio::time::timeout_at(deadline_at, child.wait()) => match result {
                Ok(Ok(status)) if status.success() => Ok(()),
                Ok(Ok(_)) => Err(ProviderFailure::GhCommand),
                Ok(Err(_)) => Err(ProviderFailure::GhUnavailable),
                Err(_) => {
                    kill_child_and_wait(child).await;
                    Err(ProviderFailure::GhTimeout)
                }
                }
            }
        }
        FirstCompletion::Child(Ok(status)) => {
            tokio::select! {
                biased;
                _ = &mut *cancel => {
                    kill_child_and_wait(child).await;
                    Err(ProviderFailure::GhOutput)
                }
                result = tokio::time::timeout_at(deadline_at, copy.as_mut()) => match result {
                Ok(Ok(())) if status.success() => Ok(()),
                Ok(Ok(())) => Err(ProviderFailure::GhCommand),
                Ok(Err(error)) => Err(error),
                Err(_) => {
                    kill_child_and_wait(child).await;
                    Err(ProviderFailure::GhTimeout)
                }
                }
            }
        }
        FirstCompletion::Copy(Err(error)) => {
            kill_child_and_wait(child).await;
            Err(error)
        }
        FirstCompletion::Child(Err(_)) => {
            kill_child_and_wait(child).await;
            Err(ProviderFailure::GhUnavailable)
        }
        FirstCompletion::Timeout => {
            kill_child_and_wait(child).await;
            Err(ProviderFailure::GhTimeout)
        }
        FirstCompletion::Cancelled => {
            kill_child_and_wait(child).await;
            Err(ProviderFailure::GhOutput)
        }
    }
}

pub(super) async fn kill_child_and_wait(child: &mut Child) {
    let _kill_result = child.start_kill();
    let _wait_result = child.wait().await;
}
