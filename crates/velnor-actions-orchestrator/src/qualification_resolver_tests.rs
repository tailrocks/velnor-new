use super::{QUALIFICATION_ADMISSION_FILENAME, stage_admission};

#[test]
fn stages_admission_beside_plan_artifact() {
    let runner_temp = tempfile::tempdir().expect("runner temp");
    let run_key = "r700-a1";
    let bytes = b"admission evidence";

    stage_admission(runner_temp.path(), run_key, bytes).expect("stage admission");

    let artifact_file = runner_temp
        .path()
        .join("velnor")
        .join(run_key)
        .join(QUALIFICATION_ADMISSION_FILENAME);
    assert_eq!(std::fs::read(artifact_file).expect("admission file"), bytes);
    assert!(
        !runner_temp
            .path()
            .join("velnor")
            .join(QUALIFICATION_ADMISSION_FILENAME)
            .exists()
    );
}
