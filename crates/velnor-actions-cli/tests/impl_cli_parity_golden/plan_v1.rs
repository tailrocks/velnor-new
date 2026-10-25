//! Internal plan-v1 invocation used by the parity golden cases.

use std::error::Error;
use std::path::Path;

/// Run `plan-v1` through the binary's internal dispatch; return the response.
pub(super) fn run(
    repo: &Path,
    head: &str,
    run_key: &str,
    has_rust: bool,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let internal = repo.join("parity-internal");
    std::fs::create_dir_all(&internal)?;
    let request = internal.join("plan-v1-request.json");
    std::fs::write(
        &request,
        serde_json::json!({
            "schema": 1,
            "run_key": run_key,
            "base": null,
            "head": head,
            "event": "local",
        })
        .to_string(),
    )?;
    let runner_temp = repo.join("runner-temp");
    std::fs::create_dir_all(&runner_temp)?;
    let github_output = internal.join("github-output.txt");
    std::fs::write(&github_output, "")?;
    let output = crate::impl_cli_tmp::spawn_for_rust_workspace(
        has_rust,
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            (
                "VELNOR_REQUEST_FILE",
                request.to_str().ok_or("non-utf8 request path")?,
            ),
            (
                "RUNNER_TEMP",
                runner_temp.to_str().ok_or("non-utf8 temp path")?,
            ),
            (
                "GITHUB_OUTPUT",
                github_output.to_str().ok_or("non-utf8 output path")?,
            ),
        ],
        repo,
    )?;
    if crate::impl_cli_tmp::code(&output) != 0 {
        return Err(format!("plan-v1 failed: {:?}", output.stderr).into());
    }
    Ok(std::fs::read(internal.join("plan-v1-response.json"))?)
}
