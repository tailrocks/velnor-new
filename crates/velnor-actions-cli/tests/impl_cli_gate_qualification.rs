//! CLI gate coverage for the read-only hosted-qualification resolver.

use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn qualification_resolver_requires_dispatch_token_and_exact_request_path()
-> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-qualification")?;
    let request_dir = tmp.join("velnor/request");
    std::fs::create_dir_all(&request_dir)?;
    let request = request_dir.join("plan-request.json");
    std::fs::write(&request, "{}")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;

    for vars in [
        vec![
            ("VELNOR_INTERNAL_OP", "resolve-qualification-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
            ("GITHUB_EVENT_NAME", "workflow_dispatch"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", "resolve-qualification-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
            ("GITHUB_EVENT_NAME", "push"),
            ("GH_TOKEN", "test-token"),
        ],
    ] {
        let gated = spawn_isolated(&[], &vars, &tmp)?;
        assert_eq!(code(&gated), 2);
        assert_identical(&bare, &gated);
    }

    let internal = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "resolve-qualification-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
            ("GITHUB_EVENT_NAME", "workflow_dispatch"),
            ("GH_TOKEN", "test-token"),
        ],
        &tmp,
    )?;
    assert_eq!(code(&internal), 1);
    assert!(internal.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&internal.stderr);
    assert!(stderr.contains("internal request failed"), "{stderr}");
    assert!(!stderr.contains("test-token"), "secret appeared in stderr");
    assert!(!stderr.contains("resolve-qualification-v1"));
    cleanup(&tmp);
    Ok(())
}
