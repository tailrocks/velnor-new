use velnor_runner_github::Poll;

use crate::journal::Journal;
use crate::scale_set::EnsureError;

use super::{slot, steps};
use crate::launch::capacity::{self, Admit, Seat};

fn progress_batch(polled: &Poll) -> bool {
    let Poll::Batch(batch) = polled else {
        return false;
    };
    crate::assign::progress_only(batch)
}

fn assigned_in(polled: &Poll) -> u32 {
    let Poll::Batch(batch) = polled else {
        return 0;
    };
    let raw = batch
        .statistics
        .as_ref()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    u32::try_from(raw.max(0)).unwrap_or(u32::MAX)
}

pub(crate) async fn admission<E: crate::stage::PairEngine + ?Sized>(
    engine: &E,
    journal: &Journal,
    capacity_limit: u32,
    target: u32,
    started: u32,
    polled: &Poll,
) -> Result<Admit, EnsureError> {
    slot::release_exited(journal, engine).await?;
    let idle = steps::idle(polled);
    let except = steps::mint_subject(polled);
    let occupied = slot::occupied_except(journal, except.as_deref()).await?;
    let running = if capacity::needs_running(idle) {
        slot::running_count(journal, engine).await?
    } else {
        0
    };
    Ok(capacity::admit(Seat {
        capacity: capacity_limit,
        target,
        started,
        occupied,
        running,
        assigned: assigned_in(polled),
        idle,
        progress: progress_batch(polled),
    }))
}
