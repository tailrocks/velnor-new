use super::OrchestratorError;
use velnor_actions_contract::ContractError;

#[test]
fn cancelled_and_unsupported_stay_distinct() {
    // `retrieve`, not `fetch`: impl_orch_f2a forbids the fetch verb
    // anywhere in orchestrator sources (offline analysis guarantee).
    let cancelled = OrchestratorError::cancelled("retrieve", "signal");
    assert!(cancelled.is_cancelled());
    assert!(!cancelled.is_unsupported());
    assert_eq!(cancelled.to_string(), "cancelled: retrieve: signal");
    let unsupported = OrchestratorError::unsupported("schema", "version 9");
    assert!(unsupported.is_unsupported());
    assert!(!unsupported.is_cancelled());
    assert_eq!(unsupported.to_string(), "unsupported: schema: version 9");
    let io = OrchestratorError::io("plan.json", "missing");
    assert!(!io.is_cancelled() && !io.is_unsupported());
}

#[test]
fn unsupported_schema_maps_to_unsupported() {
    let err = ContractError::UnsupportedSchema {
        field: "schema",
        found: "9".to_owned(),
        expected: "1",
    };
    let mapped = OrchestratorError::from(err);
    assert!(mapped.is_unsupported());
    assert_eq!(
        mapped.to_string(),
        "unsupported: schema: version 9, expected 1"
    );
    let conflict = ContractError::Collision("matrix key m-00".to_owned());
    let mapped = OrchestratorError::from(conflict);
    assert!(!mapped.is_unsupported() && !mapped.is_cancelled());
}
