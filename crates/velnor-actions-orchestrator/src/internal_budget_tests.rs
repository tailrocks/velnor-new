use std::collections::BTreeMap;

use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, PlanMatrix, PlannedPlatform,
};

use super::{MATRIX_BUDGET_BYTES, check_matrix_budget};

#[test]
fn oversize_matrix_errors_and_does_not_truncate() {
    let run = format!("echo {}", "x".repeat(MATRIX_BUDGET_BYTES));
    let entry = MatrixEntry::derive(
        "rust",
        "stack/rust/demo/clippy/default",
        &run,
        &format!("b3-{}", "a".repeat(64)),
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "clippy".to_owned(),
                ExecuteTaskRef::Single("stack/rust/demo/clippy/default".to_owned()),
            )]),
        },
        &format!("b3-{}", "b".repeat(64)),
        "local",
        "rust-demo",
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("platform"),
    )
    .expect("entry");
    let error = check_matrix_budget(&PlanMatrix {
        include: vec![entry],
    })
    .expect_err("oversize matrix must fail closed");
    let text = error.to_string();
    assert!(text.contains("matrix_budget_exceeded"), "{text}");
    assert!(text.contains("broaden"), "{text}");
    assert!(text.contains("reduce"), "{text}");
    assert!(!text.contains("truncat"), "{text}");
    let size = text
        .split("matrix_budget_exceeded:")
        .nth(1)
        .and_then(|rest| rest.split(':').next())
        .and_then(|size| size.parse::<usize>().ok())
        .expect("size");
    assert!(size > MATRIX_BUDGET_BYTES, "{size}");
}
