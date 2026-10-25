//! One turn for a connection when demand is more than shared capacity.

use super::super::polling::PollKind;

#[derive(Clone, Copy)]
pub(crate) enum BindingDisposition {
    Active,
    Removed,
}

#[derive(Clone, Copy)]
pub(crate) enum ExecutionDisposition {
    Live,
    Idle,
}

pub(crate) struct DeleteReadiness {
    pub(crate) binding: BindingDisposition,
    pub(crate) poll_kind: PollKind,
    pub(crate) drained: bool,
    pub(crate) candidate: bool,
    pub(crate) execution: ExecutionDisposition,
}

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

    pub(crate) const fn should_delete(&self, readiness: &DeleteReadiness) -> bool {
        (matches!(readiness.binding, BindingDisposition::Removed) || self.retiring)
            && matches!(readiness.poll_kind, PollKind::ZeroHeader)
            && readiness.drained
            && readiness.candidate
            && matches!(readiness.execution, ExecutionDisposition::Idle)
    }

    pub(crate) const fn rotation_complete(&self, no_connections: bool) -> bool {
        self.retiring && no_connections
    }
}
