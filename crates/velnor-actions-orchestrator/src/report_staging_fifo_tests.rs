//! Native Unix FIFO fixture for nonregular report refusal.

use std::os::unix::fs::FileTypeExt;

use super::*;

#[test]
fn fifo_report_is_rejected_without_waiting_for_a_writer() {
    let plan = fixture_plan();
    let temp = staged_plan(&plan);
    let matrix = PathBuf::from(&plan.matrix.include[0].matrix_key).join("matrix-report.json");
    let path = source_path(&temp, &matrix);
    let parent = path.parent().expect("matrix parent");
    fs::create_dir_all(parent).expect("matrix dir");
    // rustix's safe FIFO constructors exclude Apple; use the native tool
    // with fixed arguments and no inherited process environment.
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .env_clear()
        .current_dir(parent)
        .args(["-m", "600", "matrix-report.json"])
        .status()
        .expect("native FIFO fixture");
    assert!(status.success(), "native mkfifo failed: {status}");
    assert!(
        fs::symlink_metadata(&path)
            .expect("FIFO metadata")
            .file_type()
            .is_fifo()
    );
    let error = stage_reports_to("local", temp.path()).expect_err("FIFO refused");
    assert!(error.to_string().contains("report_payload_unreadable"));
    assert!(!temp.path().join("velnor/report-payload").exists());
}
