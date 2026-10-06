use super::{check_matrix_agreement, matrix_json_bytes};
use crate::workflow::plan::PlanMatrix;

#[test]
fn agreement_rejects_duplicate_keys() {
    let matrix = PlanMatrix { include: vec![] };
    let bytes = matrix_json_bytes(&matrix).expect("bytes");
    assert_eq!(check_matrix_agreement(&matrix, &bytes), Ok(()));
    let dup = br#"{"include":[],"include":[]}"#;
    assert!(check_matrix_agreement(&matrix, dup).is_err());
    assert!(check_matrix_agreement(&matrix, b"\xff").is_err());
}
