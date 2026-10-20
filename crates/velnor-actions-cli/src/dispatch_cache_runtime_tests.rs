use super::read_runtime_phase;
use velnor_actions_orchestrator::QualificationCacheRuntimePhase;

#[test]
fn generated_runtime_phase_values_parse_in_dispatcher() {
    assert_eq!(
        read_runtime_phase(|_| Some("before_import".to_owned())),
        Ok(QualificationCacheRuntimePhase::BeforeImport)
    );
    assert_eq!(
        read_runtime_phase(|_| Some("after_import".to_owned())),
        Ok(QualificationCacheRuntimePhase::AfterImport)
    );
}

#[test]
fn dispatcher_rejects_unknown_runtime_phase_values() {
    assert_eq!(
        read_runtime_phase(|_| Some("before-import".to_owned())),
        Err("qualification_cache_runtime_phase_invalid")
    );
}
