use std::sync::{Mutex, OnceLock};

use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreateCommitStage {
    Claim,
    Dispatch,
    Response,
    HelperId,
    Created,
    Started,
    Result,
    Removed,
    AbortRemoval,
    Absent,
    AbortUncertain,
    Interruption,
    ClaimInterrupted,
}

impl CreateCommitStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claim => "claim",
            Self::Dispatch => "dispatch",
            Self::Response => "response",
            Self::HelperId => "helper-id",
            Self::Created => "created",
            Self::Started => "started",
            Self::Result => "result",
            Self::Removed => "removed",
            Self::AbortRemoval => "abort-removal",
            Self::Absent => "absent",
            Self::AbortUncertain => "abort-uncertain",
            Self::Interruption => "interruption",
            Self::ClaimInterrupted => "claim-interrupted",
        }
    }
}

struct Gate {
    reservation_id: i64,
    stage: &'static str,
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

static GATE: OnceLock<Mutex<Option<Gate>>> = OnceLock::new();

pub(crate) fn arm_create_commit_gate(
    reservation_id: i64,
    stage: CreateCommitStage,
) -> Result<(oneshot::Receiver<()>, oneshot::Sender<()>), crate::error::HostError> {
    if reservation_id <= 0 {
        return Err(crate::error::HostError::Identity);
    }
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let slot = GATE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().map_err(|_| crate::error::HostError::Journal)?;
    if guard.is_some() {
        return Err(crate::error::HostError::Journal);
    }
    *guard = Some(Gate {
        reservation_id,
        stage: stage.as_str(),
        entered: entered_tx,
        release: release_rx,
    });
    Ok((entered_rx, release_tx))
}

pub(super) async fn pause_after_commit(reservation_id: i64, stage: &'static str) {
    let gate = GATE.get().and_then(|slot| {
        let mut guard = slot.lock().ok()?;
        let matches = guard
            .as_ref()
            .is_some_and(|gate| gate.reservation_id == reservation_id && gate.stage == stage);
        matches.then(|| guard.take()).flatten()
    });
    let Some(gate) = gate else { return };
    match gate.entered.send(()) {
        Ok(()) | Err(()) => {}
    }
    match gate.release.await {
        Ok(()) | Err(_) => {}
    }
}
