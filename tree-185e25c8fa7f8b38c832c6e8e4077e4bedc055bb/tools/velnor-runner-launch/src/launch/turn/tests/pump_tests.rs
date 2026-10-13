use super::super::{PollHost, pump};
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_host::worker::Started;

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
