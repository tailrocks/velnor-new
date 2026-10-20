use crate::launch::PositiveAdmission;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PollKind {
    Positive,
    ZeroHeader,
}

pub(crate) struct ControllerPollOutcome {
    pub(crate) assigned_total: Option<i64>,
    pub(crate) started_request_ids: Vec<i64>,
    pub(crate) wake_cleanup: bool,
    pub(crate) session_delete_candidate: bool,
    pub(crate) acquired: Option<crate::launch::steps::AcquiredProvisioning>,
    pub(crate) positive_poll_settled: bool,
    pub(crate) poll_kind: PollKind,
}

impl ControllerPollOutcome {
    pub(crate) fn from_scoped(outcome: crate::launch::ScopedPollOutcome, cursor: i64) -> Self {
        Self {
            assigned_total: outcome.assigned_total,
            started_request_ids: outcome.started_request_ids,
            wake_cleanup: outcome.wake_cleanup,
            // A filtered response does not prove that the queue head is empty.
            session_delete_candidate: outcome.session_delete_candidate && cursor == 0,
            acquired: None,
            positive_poll_settled: false,
            poll_kind: PollKind::ZeroHeader,
        }
    }

    pub(crate) fn from_positive(outcome: crate::launch::PositivePollOutcome) -> Self {
        let positive_poll_settled = outcome.settlement == crate::launch::Settlement::Settled;
        if let Some(PositiveAdmission::KnownNoAcquire(receipt)) = &outcome.admission {
            if receipt.state() == crate::journal::RequestAcquireState::Rejected {
                eprintln!("GitHub rejected AcquireJobs. Velnor did not send an ACK.");
            } else if !receipt.reservation_released() {
                eprintln!(
                    "Velnor kept the physical reservation because it could not prove that the job had no effects."
                );
            }
        }
        Self {
            assigned_total: outcome.assigned_total,
            started_request_ids: outcome.started_request_ids,
            wake_cleanup: outcome.wake_cleanup,
            session_delete_candidate: outcome.session_delete_candidate,
            acquired: match outcome.admission {
                Some(PositiveAdmission::Acquired(acquired)) => Some(acquired),
                None
                | Some(
                    PositiveAdmission::Deferred(_)
                    | PositiveAdmission::NoAcquire
                    | PositiveAdmission::KnownNoAcquire(_)
                    | PositiveAdmission::OwnedTerminal(_),
                ) => None,
            },
            positive_poll_settled,
            poll_kind: PollKind::Positive,
        }
    }
}
