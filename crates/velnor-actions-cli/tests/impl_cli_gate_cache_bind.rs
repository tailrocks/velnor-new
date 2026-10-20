//! CLI gate coverage for plan-bound cache-layer binding.

use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn cache_binding_requires_runner_temp_and_both_run_coordinates() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-cache-bind")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;

    for vars in [
        vec![
            ("VELNOR_INTERNAL_OP", "bind-qualification-cache-v1"),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", "bind-qualification-cache-v1"),
            ("RUNNER_TEMP", tmp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ],
    ] {
        let gated = spawn_isolated(&[], &vars, &tmp)?;
        assert_identical(&bare, &gated);
    }

    let internal = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "bind-qualification-cache-v1"),
            ("RUNNER_TEMP", tmp.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("GITHUB_REF_PROTECTED", "false"),
            ("VELNOR_QUALIFICATION_CACHE_OWNER_ID", "rust-demo"),
            ("VELNOR_QUALIFICATION_CACHE_LAYER", "mbx_objects"),
            ("VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE", "before_import"),
        ],
        &tmp,
    )?;
    assert_eq!(code(&internal), 1);
    assert_eq!(internal.stdout, [] as [u8; 0]);
    let stderr = String::from_utf8_lossy(&internal.stderr);
    assert!(
        stderr.contains("cache binding failed: missing_github_event_name"),
        "{stderr}"
    );
    assert!(!stderr.contains("bind-qualification-cache-v1"));
    cleanup(&tmp);
    Ok(())
}
