use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::super::slot;
use super::Turn;

pub(super) trait PollHost {
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError>;

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
