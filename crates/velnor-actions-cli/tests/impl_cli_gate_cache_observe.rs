//! CLI gate coverage for cache-backend observation.

use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn cache_observation_requires_runner_temp_and_both_run_coordinates() -> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("gate-cache-observe")?;
    let bare = spawn_isolated(&[], &[], &temp)?;

    for vars in [
        vec![
            (
                "VELNOR_INTERNAL_OP",
                "observe-qualification-cache-backend-v1",
            ),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            (
                "VELNOR_INTERNAL_OP",
                "observe-qualification-cache-backend-v1",
            ),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            (
                "VELNOR_INTERNAL_OP",
                "observe-qualification-cache-backend-v1",
            ),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "700"),
        ],
    ] {
        let gated = spawn_isolated(&[], &vars, &temp)?;
        assert_identical(&bare, &gated);
    }

    let internal = spawn_isolated(
        &[],
        &[
            (
                "VELNOR_INTERNAL_OP",
                "observe-qualification-cache-backend-v1",
            ),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("GITHUB_REF_PROTECTED", "false"),
            ("VELNOR_QUALIFICATION_RUNTIME_IDENTITY", "{}"),
        ],
        &temp,
    )?;
    assert_eq!(code(&internal), 1);
    assert_eq!(internal.stdout, [] as [u8; 0]);
    let stderr = String::from_utf8_lossy(&internal.stderr);
    assert!(stderr.contains("missing_github_event_name"), "{stderr}");
    assert!(!stderr.contains("observe-qualification-cache-backend-v1"));
    cleanup(&temp);
    Ok(())
}
