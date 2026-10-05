use std::path::Path;

use super::admission_path;
use crate::qualification_admission::QUALIFICATION_ADMISSION_FILENAME;

#[test]
fn admission_path_matches_the_generated_plan_request_name() {
    let path = Path::new("/runner-temp/velnor/request/plan-v1-request.json");
    assert!(admission_path(path).is_ok_and(|actual| {
        actual == Path::new("/runner-temp/velnor/request/qualification-admission.json")
    }));
    assert_eq!(
        QUALIFICATION_ADMISSION_FILENAME,
        "qualification-admission.json"
    );
}

#[test]
fn admission_path_rejects_other_operation_and_legacy_names() {
    for path in [
        Path::new("/runner-temp/velnor/request/merge-v1-request.json"),
        Path::new("/runner-temp/velnor/request/plan-request.json"),
    ] {
        assert!(admission_path(path).is_err(), "accepted {path:?}");
    }
}
