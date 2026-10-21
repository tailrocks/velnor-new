//! CLI gate coverage for qualification-cache observations.

use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

const RUNTIME_OP: &str = "observe-qualification-cache-runtime-v1";
const RETIRED_BACKEND_OP: &str = "observe-qualification-cache-backend-v1";

#[test]
fn cache_runtime_observation_requires_runner_temp_and_both_run_coordinates()
-> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("gate-cache-observe")?;
    let bare = spawn_isolated(&[], &[], &temp)?;

    for vars in [
        vec![
            ("VELNOR_INTERNAL_OP", RUNTIME_OP),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", RUNTIME_OP),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", RUNTIME_OP),
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
            ("VELNOR_INTERNAL_OP", RUNTIME_OP),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("GITHUB_REF_PROTECTED", "false"),
            ("VELNOR_QUALIFICATION_CACHE_OWNER_ID", "rust-demo"),
            ("VELNOR_QUALIFICATION_CACHE_LAYER", "mbx_objects"),
            ("VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE", "before_import"),
        ],
        &temp,
    )?;
    assert_eq!(code(&internal), 1);
    assert_eq!(internal.stdout, [] as [u8; 0]);
    let stderr = String::from_utf8_lossy(&internal.stderr);
    assert!(stderr.contains("missing_github_event_name"), "{stderr}");

    let retired = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", RETIRED_BACKEND_OP),
            ("RUNNER_TEMP", temp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        &temp,
    )?;
    assert_eq!(code(&retired), 2);
    assert_identical(&bare, &retired);
    cleanup(&temp);
    Ok(())
}
