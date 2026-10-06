//! Poll until admission stops and no owned launch container is running.

use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::super::slot;
use super::Turn;

/// One session's poll and running-count source.
pub(super) trait PollHost {
    /// `Ok(false)` keeps the session. `Ok(true)` is an admission stop.
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError>;

    /// Owned containers still running.
    async fn running(&mut self) -> Result<u32, EnsureError>;
}

impl PollHost for Turn<'_> {
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        self.drive_poll(workers).await
    }

    async fn running(&mut self) -> Result<u32, EnsureError> {
        slot::running_count(self.journal, self.docker).await
    }
}

/// Keep polling while an owned container runs. An empty session stays open past `bound`.
pub(super) async fn until_idle(
    turn: &mut Turn<'_>,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    pump(turn, workers, bound).await
}

pub(super) async fn pump<H: PollHost>(
    host: &mut H,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    let mut polls = 0usize;
    let mut missed = 0u8;
    loop {
        if polls >= bound && !workers.is_empty() && missed >= 2 && host.running().await? == 0 {
            return Ok(());
        }
        let stop = host.poll(workers).await?;
        polls = polls.saturating_add(1);
        if !stop {
            // The broker can assign a job only while this session still exists.
            if workers.is_empty() {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            continue;
        }
        if host.running().await? > 0 {
            missed = 0;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }
        missed = missed.saturating_add(1);
        if workers.is_empty() || missed >= 2 {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{PollHost, pump};
    use crate::scale_set::EnsureError;
    use crate::worker::Started;

    struct Fake {
        polls: usize,
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "test poll host implements the async interface without I/O"
    )]
    impl PollHost for Fake {
        async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
            let _ = workers;
            self.polls = self.polls.saturating_add(1);
            Ok(self.polls >= 10)
        }

        async fn running(&mut self) -> Result<u32, EnsureError> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn empty_polls_keep_the_same_session() -> Result<(), String> {
        let mut host = Fake { polls: 0 };
        let mut workers = Vec::new();
        pump(&mut host, &mut workers, 8)
            .await
            .map_err(|err| err.to_string())?;
        // Bound 8 used to return before this poll. `launch_once` deletes only after return.
        assert!(host.polls >= 9, "{}", host.polls);
        Ok(())
    }

    #[test]
    fn cleared_name_keeps_the_session() {
        assert!(super::super::queue_stays(Some(&EnsureError::NameCleared)));
        assert!(super::super::queue_stays(Some(&EnsureError::NameSteady)));
        assert!(!super::super::queue_stays(Some(&EnsureError::Conflict)));
        assert!(!super::super::queue_stays(None));
    }
}
