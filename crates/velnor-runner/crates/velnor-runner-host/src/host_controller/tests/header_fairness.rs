use crate::config_test_support::{basic_connection, host_config};
use crate::journal::CapacityLimits;
use crate::launch::{CompletedHeaderPoll, DeletedSession204};
use crate::{HostConfig, HostError, Journal};

use super::requested_header_capacity_after;
use crate::host_controller::loop_driver::{FairnessTurn, HeaderPollPlan, PollKind};

#[tokio::test]
async fn production_poll_turn_rotates_durable_sessions_after_drain_and_exact_delete()
-> Result<(), Box<dyn std::error::Error>> {
    let config = HostConfig::parse(&host_config(
        &[
            basic_connection("alpha", 101, "owner/one", "one", "one-set"),
            basic_connection("beta", 202, "owner/two", "two", "two-set"),
        ],
        1,
    ))?;
    let scratch = HeaderScratch::new()?;
    let journal = open_journal(&scratch).await?;
    let alpha = config.connections()[0].connection_id.clone();
    let beta = config.connections()[1].connection_id.clone();
    let positive = journal
        .reserve_header_allocation(alpha.clone(), 101, "session-a", 1)
        .await?;
    let waiting = journal
        .reserve_header_allocation(beta.clone(), 202, "session-b", 0)
        .await?;

    // This test uses the controller turn from `poll_active_connections`.
    // A poll with a positive capacity header ends admission for this pass.
    let mut first_turn = FairnessTurn::new(true, false);
    assert_eq!(
        first_turn.poll_plan(false, positive.target(), || 91),
        HeaderPollPlan::Positive
    );
    let positive_poll = journal.begin_header_poll(&positive).await?;
    assert_eq!(positive_poll.header_value(), 1);
    journal
        .settle_header_poll(CompletedHeaderPoll::test_only(positive_poll))
        .await?;
    first_turn.observe_positive_poll(true);
    assert_eq!(
        first_turn.poll_plan(false, waiting.target(), || 44),
        HeaderPollPlan::Zero { cursor: 0 }
    );

    // The mock first returns an Available request. Do not delete its session.
    // The next empty response has a zero header.
    let available_head_deferred = true;
    let zero_poll = journal.begin_zero_header_poll(&waiting).await?;
    assert_eq!(zero_poll.header_value(), 0);
    journal
        .settle_header_poll(CompletedHeaderPoll::test_only(zero_poll))
        .await?;
    assert!(!first_turn.should_delete(
        false,
        PollKind::ZeroHeader,
        true,
        !available_head_deferred,
        true,
    ));
    let empty_zero_poll = journal.begin_zero_header_poll(&waiting).await?;
    journal
        .settle_header_poll(CompletedHeaderPoll::test_only(empty_zero_poll))
        .await?;
    assert!(first_turn.should_delete(false, PollKind::ZeroHeader, true, true, true,));
    let delete = journal.begin_header_session_delete(&waiting).await?;
    journal
        .retire_header_allocation(DeletedSession204::test_only(delete))
        .await?;
    assert!(!first_turn.rotation_complete(false));
    drop(journal);

    // The test opens the journal again between turns. The allocation and cursor
    // remain after the zero-capacity connection stops.
    let reopened = open_journal(&scratch).await?;
    assert_eq!(
        reopened.header_fairness_cursor().await?,
        Some(alpha.clone())
    );
    assert_eq!(reopened.charged_header_capacity().await?, 1);
    let allocations = reopened.header_allocations().await?;
    assert_eq!(allocations.len(), 1);
    assert_eq!(allocations[0].connection_id(), &alpha);
    let mut retiring_turn = FairnessTurn::new(true, true);
    assert_eq!(
        retiring_turn.poll_plan(false, positive.target(), || 17),
        HeaderPollPlan::Zero { cursor: 0 }
    );
    let alpha_zero = reopened.begin_zero_header_poll(&positive).await?;
    reopened
        .settle_header_poll(CompletedHeaderPoll::test_only(alpha_zero))
        .await?;
    let alpha_delete = reopened.begin_header_session_delete(&positive).await?;
    reopened
        .retire_header_allocation(DeletedSession204::test_only(alpha_delete))
        .await?;
    retiring_turn.observe_positive_poll(false);
    assert!(retiring_turn.rotation_complete(true));
    drop(reopened);

    // Setup reads the saved cursor. It gives the next header to beta.
    let next_journal = open_journal(&scratch).await?;
    let cursor = next_journal.header_fairness_cursor().await?;
    let next = config
        .connections()
        .iter()
        .map(|binding| requested_header_capacity_after(&config, binding, cursor.as_ref()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(next, [0, 1]);
    let next_positive = next_journal
        .reserve_header_allocation(beta, 202, "session-c", 1)
        .await?;
    let uncertain_turn = FairnessTurn::new(true, true);
    assert_eq!(
        uncertain_turn.poll_plan(false, next_positive.target(), || 53),
        HeaderPollPlan::Zero { cursor: 0 }
    );
    let beta_zero = next_journal.begin_zero_header_poll(&next_positive).await?;
    next_journal
        .settle_header_poll(CompletedHeaderPoll::test_only(beta_zero))
        .await?;
    assert!(!uncertain_turn.should_delete(false, PollKind::ZeroHeader, false, true, true,));
    assert!(uncertain_turn.should_delete(false, PollKind::ZeroHeader, true, true, true,));
    let unknown = next_journal
        .begin_header_session_delete(&next_positive)
        .await?;
    next_journal.mark_header_delete_uncertain(unknown).await?;
    assert_eq!(next_journal.charged_header_capacity().await?, 1);
    assert!(
        next_journal
            .reserve_header_allocation(alpha, 101, "session-d", 1)
            .await
            .is_err()
    );
    Ok(())
}

struct HeaderScratch(std::path::PathBuf);

impl HeaderScratch {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "velnor-controller-fairness-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn journal_path(&self) -> std::path::PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for HeaderScratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

async fn open_journal(scratch: &HeaderScratch) -> Result<Journal, HostError> {
    let journal = Journal::open(&scratch.journal_path()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await?;
    journal.bootstrap_header_authority().await?;
    Ok(journal)
}
