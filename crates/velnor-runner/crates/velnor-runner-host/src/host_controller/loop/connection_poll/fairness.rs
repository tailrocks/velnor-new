//! One turn for a connection when demand is more than shared capacity.

use super::super::polling::PollKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderPollPlan {
    Positive,
    Zero { cursor: i64 },
}

pub(crate) struct FairnessTurn {
    enabled: bool,
    retiring: bool,
}

impl FairnessTurn {
    pub(crate) const fn new(enabled: bool, rotating: bool) -> Self {
        Self {
            enabled,
            retiring: rotating,
        }
    }

    pub(crate) fn poll_plan(
        &self,
        removed: bool,
        target: u32,
        cursor: impl FnOnce() -> i64,
    ) -> HeaderPollPlan {
        if !removed && target > 0 && !self.retiring {
            HeaderPollPlan::Positive
        } else {
            HeaderPollPlan::Zero {
                cursor: if removed || self.retiring {
                    0
                } else {
                    cursor()
                },
            }
        }
    }

    pub(crate) const fn retiring(&self) -> bool {
        self.retiring
    }

    pub(crate) fn observe_positive_poll(&mut self, settled: bool) {
        if self.enabled && settled {
            self.retiring = true;
        }
    }

    pub(crate) const fn should_delete(
        &self,
        removed: bool,
        poll_kind: PollKind,
        drained: bool,
        delete_candidate: bool,
        no_live_execution: bool,
    ) -> bool {
        (removed || self.retiring)
            && matches!(poll_kind, PollKind::ZeroHeader)
            && drained
            && delete_candidate
            && no_live_execution
    }

    pub(crate) const fn rotation_complete(&self, no_connections: bool) -> bool {
        self.retiring && no_connections
    }
}
