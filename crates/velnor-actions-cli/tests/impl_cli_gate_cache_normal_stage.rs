//! Private gate coverage for the normal tools cache stage path.

use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};
use std::path::Path;

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

#[test]
fn normal_cache_stage_writes_the_v3_key_path_to_the_owned_output() -> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("normal-cache-stage-path")?;
    let temp = std::fs::canonicalize(temp)?;
    let output_path = temp.join("github-output");
    std::fs::write(&output_path, "prior=value\n")?;
    let runner_temp = temp.to_str().ok_or("runner temp is not UTF-8")?;
    let output = output_path.to_str().ok_or("output path is not UTF-8")?;
    let identity = "a".repeat(64);
    let key = format!("mise-tools-v2-payload-v3-{identity}");
    let result = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "normal-tools-cache-stage-path-v1"),
            ("RUNNER_TEMP", runner_temp),
            ("GITHUB_RUN_ID", "700"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_NORMAL_CACHE_KEY", &key),
            ("GITHUB_OUTPUT", output),
        ],
        &temp,
    )?;

    assert_eq!(code(&result), 0, "{:?}", result.stderr);
    let output_text = std::fs::read_to_string(&output_path)?;
    let line = output_text
        .strip_prefix("prior=value\n")
        .ok_or("prior output changed")?;
    let stage_path = line
        .strip_prefix("stage_path=")
        .and_then(|value| value.strip_suffix('\n'))
        .ok_or("stage path output missing")?;
    let expected_parent = temp.join("velnor/normal-cache-v3/mise_tools");
    let relative = Path::new(stage_path)
        .strip_prefix(&expected_parent)
        .map_err(|_| "stage path has a different root")?;
    let mut components = relative.components();
    let digest = components
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .ok_or("stage path digest missing")?;
    assert_eq!(
        components.next().and_then(|part| part.as_os_str().to_str()),
        Some("payload")
    );
    assert!(components.next().is_none());
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    cleanup(&temp);
    Ok(())
}
