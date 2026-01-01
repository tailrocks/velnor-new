use super::*;

pub(super) fn write_plan(runner_temp: &Path, plan: &Plan) {
    fs::write(
        runner_temp.join("velnor").join(RUN_KEY).join("plan.json"),
        serde_json::to_vec(plan).expect("plan JSON"),
    )
    .expect("plan file");
}

pub(super) fn nul_fields(frame: &[u8]) -> Vec<&str> {
    assert_eq!(frame.last(), Some(&0), "frame ends with a NUL");
    frame
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| std::str::from_utf8(field).expect("UTF-8 frame field"))
        .collect()
}

pub(super) fn assert_error(error: &OrchestratorError, expected: &str) {
    assert!(
        error.to_string().contains(expected),
        "expected {expected:?}, got {error}"
    );
}

pub(super) fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}
