use super::*;

impl FakeEngine {
    pub(in crate::worker::cleanup::tests) fn running_with_lost_dind_stop_response() -> Self {
        let engine = Self::running();
        *engine.lose_dind_stop_response.lock().expect("mutex") = true;
        engine
    }

    pub(in crate::worker::cleanup::tests) fn running_with_dind_stop_failure() -> Self {
        let engine = Self::running();
        *engine.fail_dind_stop_before_effect.lock().expect("mutex") = true;
        engine
    }
}

pub(super) async fn fake_stop_dind(engine: &FakeEngine) -> Result<DindStopEvidence, HostError> {
    engine.push("stop-dind");
    let failed_before_effect = {
        let mut fail = engine.fail_dind_stop_before_effect.lock().expect("mutex");
        let value = *fail;
        *fail = false;
        value
    };
    if failed_before_effect {
        return Err(HostError::Docker);
    }
    *engine.dind_running.lock().expect("mutex") = false;
    let response_lost = {
        let mut fail = engine.lose_dind_stop_response.lock().expect("mutex");
        let value = *fail;
        *fail = false;
        value
    };
    if response_lost {
        Err(HostError::Docker)
    } else {
        Ok(DindStopEvidence { stopped: true })
    }
}
