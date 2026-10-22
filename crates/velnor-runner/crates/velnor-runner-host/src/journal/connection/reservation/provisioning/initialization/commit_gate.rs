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
    CreateUncertain,
}

impl CreateCommitStage {
    fn parse(stage: &str) -> Option<Self> {
        Some(match stage {
            "claim" => Self::Claim,
            "dispatch" => Self::Dispatch,
            "response" => Self::Response,
            "helper-id" => Self::HelperId,
            "created" => Self::Created,
            "started" => Self::Started,
            "result" => Self::Result,
            "removed" => Self::Removed,
            "abort-removal" => Self::AbortRemoval,
            "absent" => Self::Absent,
            "abort-uncertain" => Self::AbortUncertain,
            "interruption" => Self::Interruption,
            "claim-interrupted" => Self::ClaimInterrupted,
            "create-uncertain" => Self::CreateUncertain,
            _ => return None,
        })
    }
}

struct Gate {
    reservation_id: i64,
    stage: CreateCommitStage,
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
        stage,
        entered: entered_tx,
        release: release_rx,
    });
    Ok((entered_rx, release_tx))
}

pub(super) async fn pause_after_commit(reservation_id: i64, stage: &'static str) {
    let Some(stage) = CreateCommitStage::parse(stage) else {
        return;
    };
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

#[cfg(test)]
mod tests {
    use super::CreateCommitStage;

    #[test]
    fn parser_covers_commit_hook_stages_and_rejects_unknown_names() {
        let stages = [
            ("claim", CreateCommitStage::Claim),
            ("dispatch", CreateCommitStage::Dispatch),
            ("response", CreateCommitStage::Response),
            ("helper-id", CreateCommitStage::HelperId),
            ("created", CreateCommitStage::Created),
            ("started", CreateCommitStage::Started),
            ("result", CreateCommitStage::Result),
            ("removed", CreateCommitStage::Removed),
            ("abort-removal", CreateCommitStage::AbortRemoval),
            ("absent", CreateCommitStage::Absent),
            ("abort-uncertain", CreateCommitStage::AbortUncertain),
            ("interruption", CreateCommitStage::Interruption),
            ("claim-interrupted", CreateCommitStage::ClaimInterrupted),
            ("create-uncertain", CreateCommitStage::CreateUncertain),
        ];

        for (name, stage) in stages {
            assert_eq!(CreateCommitStage::parse(name), Some(stage));
        }
        assert_eq!(CreateCommitStage::parse("unknown"), None);
    }
}
