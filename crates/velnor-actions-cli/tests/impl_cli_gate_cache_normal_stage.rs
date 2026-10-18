//! Private gate coverage for the normal tools cache stage path.

use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn normal_cache_stage_requires_runner_temp_and_canonical_run_coordinates()
-> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("gate-cache-normal-stage")?;
    let request = temp.join("untrusted-request.json");
    std::fs::write(&request, "not a stage-path input")?;
    let bare = spawn_isolated(&[], &[], &temp)?;

    for vars in [
        vec![
            ("VELNOR_INTERNAL_OP", "normal-tools-cache-stage-path-v1"),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", "normal-tools-cache-stage-path-v1"),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", "normal-tools-cache-stage-path-v1"),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "invalid"),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", "normal-tools-cache-stage-path-v1"),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "01"),
        ],
    ] {
        let mut with_request = vars;
        with_request.push(("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")));
        let gated = spawn_isolated(&[], &with_request, &temp)?;
        assert_identical(&bare, &gated);
    }

    let entered = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "normal-tools-cache-stage-path-v1"),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
        ],
        &temp,
    )?;
    assert_eq!(code(&entered), 1);
    assert_eq!(entered.stdout, [] as [u8; 0]);
    let stderr = String::from_utf8_lossy(&entered.stderr);
    assert!(
        stderr.contains("normal tools cache stage failed: normal_cache_key_missing"),
        "{stderr}"
    );
    assert!(!stderr.contains("normal-tools-cache-stage-path-v1"));
    cleanup(&temp);
    Ok(())
}
