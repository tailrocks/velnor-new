//! Unknown-key rejection for the workflow wire types.
//!
//! Homed here because `report.rs` is at the file-size gate; the
//! task/matrix types live there, `RequiredJobResult` lives here.

use serde::de::DeserializeOwned;
use serde::ser::Serialize;

use super::{JobConclusion, RequiredJobResult};
use crate::workflow::plan::WorkflowEvent;
use crate::workflow::report::{
    CacheLayer, CacheOutcome, CacheResult, MatrixReport, MatrixStatus, MatrixTaskEntry, TaskReport,
    TaskStatus,
};
use crate::workflow::trust::Trust;

/// Round-trip the value, then prove one unknown key rejects.
fn rejects_unknown(value: &impl Serialize, typed: fn(serde_json::Value) -> bool) {
    let mut wire = serde_json::to_value(value).expect("serialize");
    assert!(typed(wire.clone()), "valid wire must parse");
    wire["velnor_unknown_probe"] = serde_json::json!(1);
    assert!(!typed(wire), "unknown key must reject");
}

/// Parse probe for one wire type.
fn parses<T: DeserializeOwned>(wire: serde_json::Value) -> bool {
    serde_json::from_value::<T>(wire).is_ok()
}

#[test]
fn task_wire_rejects_unknown_keys() {
    let task = TaskReport {
        schema: TaskReport::SCHEMA,
        task_report_id: "task-local-m-0123456789abcdef-0123456789abcdef".to_owned(),
        run_key: "local".to_owned(),
        event: WorkflowEvent::Push,
        trust: Trust::Trusted,
        matrix_id: "stack:rust|task:internal/plan/default".to_owned(),
        matrix_key: "m-0123456789abcdef".to_owned(),
        task_id: "internal/plan/default".to_owned(),
        task_digest: "b3-00".to_owned(),
        status: TaskStatus::Failed,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: String::new(),
            result: CacheResult::NotAttempted,
            miss_reason: None,
        },
        exit_code: 1,
        duration_ms: None,
        outputs: Vec::new(),
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    };
    rejects_unknown(&task.cache, parses::<CacheOutcome>);
    rejects_unknown(&task, parses::<TaskReport>);
}

#[test]
fn matrix_wire_rejects_unknown_keys() {
    let entry = MatrixTaskEntry {
        task_report_id: "task-local-m-0123456789abcdef-0123456789abcdef".to_owned(),
        task_id: "internal/plan/default".to_owned(),
        status: TaskStatus::Failed,
        exit_code: 1,
    };
    let matrix = MatrixReport {
        schema: MatrixReport::SCHEMA,
        report_id: "report-local-m-0123456789abcdef".to_owned(),
        run_key: "local".to_owned(),
        matrix_id: "stack:rust|task:internal/plan/default".to_owned(),
        matrix_key: "m-0123456789abcdef".to_owned(),
        status: MatrixStatus::Failed,
        expected_task_ids: Vec::new(),
        task_report_ids: Vec::new(),
        tasks: vec![entry],
        selected: 1,
        reused: 0,
        executed: 0,
        empty_partition: 0,
        not_selected: 0,
        failed: 1,
        cancelled: 0,
    };
    rejects_unknown(&matrix.tasks[0], parses::<MatrixTaskEntry>);
    rejects_unknown(&matrix, parses::<MatrixReport>);
    rejects_unknown(
        &RequiredJobResult {
            job_id: "plan".to_owned(),
            conclusion: JobConclusion::Success,
        },
        parses::<RequiredJobResult>,
    );
}
