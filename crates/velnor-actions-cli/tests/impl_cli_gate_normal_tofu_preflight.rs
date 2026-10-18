//! Binary gate coverage for normal `Tofu` cache preflight.

use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

const OP: &str = "preflight-normal-tofu-providers-cache-restore-v1";

#[test]
fn normal_tofu_preflight_requires_runner_and_run_coordinates() -> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("gate-normal-tofu-preflight")?;
    let bare = spawn_isolated(&[], &[], &temp)?;
    let runner_temp = temp.to_str().unwrap_or("/");

    for vars in [
        vec![("VELNOR_INTERNAL_OP", OP), ("GITHUB_RUN_ID", "700")],
        vec![
            ("VELNOR_INTERNAL_OP", OP),
            ("RUNNER_TEMP", runner_temp),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", OP),
            ("RUNNER_TEMP", runner_temp),
            ("GITHUB_RUN_ID", "700"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", OP),
            ("RUNNER_TEMP", runner_temp),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "01"),
        ],
    ] {
        let gated = spawn_isolated(&[], &vars, &temp)?;
        assert_identical(&bare, &gated);
    }

    let output_path = temp.join("github-output");
    std::fs::write(&output_path, "before=kept\n")?;
    let output = output_path.to_str().unwrap_or("/");
    let entered = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", OP),
            ("GITHUB_ACTIONS", "true"),
            ("RUNNER_TEMP", runner_temp),
            ("GITHUB_WORKSPACE", runner_temp),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("GITHUB_OUTPUT", output),
        ],
        &temp,
    )?;
    assert_eq!(code(&entered), 1);
    assert_eq!(entered.stdout, [] as [u8; 0]);
    assert_eq!(std::fs::read_to_string(&output_path)?, "before=kept\n");
    let stderr = String::from_utf8_lossy(&entered.stderr);
    assert!(
        stderr.contains("normal_tofu_preflight_input_missing"),
        "{stderr}"
    );
    assert!(!stderr.contains(OP));
    cleanup(&temp);
    Ok(())
}
