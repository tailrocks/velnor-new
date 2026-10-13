use super::super::super::{
    ArtifactId, ManifestKey, MatrixId, MatrixKey, PlanId, ReportId, RunKey, TargetKey, TaskId,
    TaskReportId,
};
use super::super::validate_artifact_id;
use super::super::{artifact_id_for_crate_job, artifact_id_for_plan, target_key};
use crate::canonical::{Digest, PosixPath, digest_b3, digest_b3_typed};

#[test]
fn newtypes_accept_valid_and_reject_invalid() {
    check_id!(RunKey, "local", "Local");
    check_id!(RunKey, "r12-a3", "r-a");
    check_id!(ManifestKey, "root", "Root");
    check_id!(ManifestKey, "crates/foo", "../escape");
    check_id!(TaskId, "stack/rust/root/clippy/default", "stack/RUST");
    check_id!(TaskId, "internal/plan/default", "internal/only");
    check_id!(
        MatrixId,
        "stack:rust|task:stack/rust/root/clippy/default",
        "stack:rust"
    );
    check_id!(MatrixKey, "m-0123456789abcdef", "m-XYZ");
    check_id!(PlanId, "plan-local", "plan-");
    check_id!(ReportId, "report-local-m-0123456789abcdef", "report-x");
    check_id!(
        TaskReportId,
        "task-local-m-0123456789abcdef-0123456789abcdef",
        "task-local"
    );
    check_id!(ArtifactId, "velnor-plan-local", "velnor-nope-x");
    check_id!(TargetKey, "x86-64-unknown-linux-gnu", "bad--key");
    assert!(target_key("X86_64-Unknown-Linux-GNU").is_ok_and(|k| k == "x86-64-unknown-linux-gnu"));
    assert_eq!(
        artifact_id_for_plan("local").expect("plan"),
        "velnor-plan-local"
    );
}

#[test]
fn crate_job_artifacts_derive_and_validate_per_job() {
    let id = artifact_id_for_crate_job("local", "rust-demo").expect("derive");
    assert_eq!(id, "velnor-crate-local-rust-demo");
    assert_eq!(
        artifact_id_for_crate_job("r7-a2", "plan").expect("plan job"),
        "velnor-crate-r7-a2-plan"
    );
    assert!(validate_artifact_id(&id).is_ok());
    assert!(validate_artifact_id("velnor-crate-r7-a2-rust-demo").is_ok());
    for bad in [
        "velnor-crate-local",
        "velnor-crate-",
        "velnor-crate-local-",
        "velnor-crate-Local-rust-demo",
        "velnor-crate-local-RUST-DEMO",
        "velnor-crate-local-velnor-plan",
    ] {
        assert!(validate_artifact_id(bad).is_err(), "accepted {bad}");
    }
    assert!(artifact_id_for_crate_job("Local", "rust-demo").is_err());
    assert!(artifact_id_for_crate_job("local", "").is_err());
    assert!(artifact_id_for_crate_job("local", "velnor-plan").is_err());
}

#[test]
fn digest_and_path_newtypes_validate() {
    let raw = digest_b3(b"bytes");
    let typed = digest_b3_typed(b"bytes");
    assert_eq!(typed.as_str(), raw);
    assert_eq!(typed.prefix16().len(), 16);
    check_id!(Digest, &raw, "b3-not-hex");
    assert!(Digest::parse("B3-AAAA").is_err());
    let path = PosixPath::parse("crates/foo").expect("posix");
    assert_eq!(path.as_str(), "crates/foo");
    assert_eq!(PosixPath::parse("a\\b").expect("slash").as_str(), "a/b");
    assert!(PosixPath::parse("/abs").is_err());
    assert!(PosixPath::parse("a/../b").is_err());
    assert!(PosixPath::parse("").is_err());
    let json = serde_json::to_string(&path).expect("serialize");
    assert_eq!(serde_json::from_str::<PosixPath>(&json).expect("de"), path);
    assert!(serde_json::from_str::<PosixPath>("\"/abs\"").is_err());
}

#[test]
fn canonical_hex_pins_charset_and_rejects_empty_at_width() {
    use super::super::super::{is_lower_hex, is_lower_hex_len, validate_matrix_key};
    assert!(is_lower_hex("0123456789abcdef"));
    assert!(is_lower_hex(""), "charset-only vacuity is deliberate");
    for bad in ["ABCDEF", "ab cd", "ab\n", "xyz"] {
        assert!(!is_lower_hex(bad), "charset must reject {bad:?}");
    }
    assert!(is_lower_hex_len(&"a".repeat(40), 40));
    assert!(!is_lower_hex_len("", 40), "empty fails every width");
    assert!(!is_lower_hex_len(&"a".repeat(39), 40));
    assert!(!is_lower_hex_len(&"A".repeat(40), 40));
    assert!(validate_matrix_key("m-").is_err(), "empty hex fails");
    assert!(validate_matrix_key("m-0123456789ABCDEF").is_err());
}
