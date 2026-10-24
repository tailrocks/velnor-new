//! Private gate coverage for the Required-job cache receipt writer.

use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn cache_receipt_requires_both_run_coordinates_and_enters_only_with_them()
-> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("gate-cache-receipt")?;
    let bare = spawn_isolated(&[], &[], &temp)?;
    let temp_text = temp.to_str().ok_or("temp path is not UTF-8")?;
    for vars in [
        vec![
            ("VELNOR_INTERNAL_OP", "write-qualification-cache-receipt-v1"),
            ("RUNNER_TEMP", temp_text),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", "write-qualification-cache-receipt-v1"),
            ("RUNNER_TEMP", temp_text),
            ("GITHUB_RUN_ID", "700"),
        ],
    ] {
        let gated = spawn_isolated(&[], &vars, &temp)?;
        assert_eq!(code(&gated), 2);
        assert_identical(&bare, &gated);
    }

    let entered = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "write-qualification-cache-receipt-v1"),
            ("RUNNER_TEMP", temp_text),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        &temp,
    )?;
    assert_eq!(code(&entered), 1);
    assert_eq!(entered.stdout, [] as [u8; 0]);
    let stderr = String::from_utf8_lossy(&entered.stderr);
    assert!(stderr.contains("internal request failed"), "{stderr}");
    assert!(!stderr.contains("write-qualification-cache-receipt-v1"));
    cleanup(&temp);
    Ok(())
}
