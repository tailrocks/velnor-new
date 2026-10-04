//! Private MBX store initialization regressions.

use std::error::Error;
use std::fs;

use super::{
    ATTEMPT, JOB_ID, RUN_ID, STORE_INIT_SCRIPT, Sandbox, github_env_value, init_root,
    init_root_with_matrix, run_script,
};

#[test]
fn init_creates_distinct_private_roots_with_exact_owner_identity() -> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let first = init_root(&sandbox, ATTEMPT)?;
    let second = init_root(&sandbox, ATTEMPT)?;
    let canonical_temp = fs::canonicalize(sandbox.path())?;
    assert_ne!(first, second);
    assert_eq!(first.parent(), Some(canonical_temp.as_path()));
    assert_eq!(second.parent(), Some(canonical_temp.as_path()));
    assert_eq!(
        github_env_value(&sandbox, ATTEMPT, "MBX_CACHE_EXPORT_GROUP")?,
        format!("velnor-{RUN_ID}-{ATTEMPT}-{JOB_ID}-nonmatrix")
    );
    let init_output =
        fs::read_to_string(sandbox.path().join(format!("store-init-output-{ATTEMPT}")))?;
    assert!(init_output.contains("ready=true"), "{init_output}");
    let marker = fs::read_to_string(first.join(".velnor-mbx-owner"))?;
    assert_eq!(
        marker,
        format!("run_id={RUN_ID}\njob={JOB_ID}\nattempt={ATTEMPT}\n")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&first)?.permissions().mode() & 0o777, 0o700);
    }
    let matrix = init_root_with_matrix(&sandbox, ATTEMPT, "m-0123456789abcdef")?;
    assert_ne!(second, matrix);
    assert_eq!(
        github_env_value(&sandbox, ATTEMPT, "MBX_CACHE_EXPORT_GROUP")?,
        format!("velnor-{RUN_ID}-{ATTEMPT}-{JOB_ID}-m-0123456789abcdef")
    );
    Ok(())
}

#[test]
fn failed_env_handoff_reports_cache_unavailable_without_failing_setup() -> Result<(), Box<dyn Error>>
{
    let sandbox = Sandbox::create()?;
    let output_file = sandbox.path().join("store-init-failed-output");
    fs::write(&output_file, "")?;
    let temp = sandbox.path().to_string_lossy().into_owned();
    let output_path = output_file.to_string_lossy().into_owned();
    let output = run_script(
        STORE_INIT_SCRIPT,
        &[
            ("RUNNER_TEMP", &temp),
            ("GITHUB_ENV", &temp),
            ("GITHUB_OUTPUT", &output_path),
            ("GITHUB_RUN_ID", RUN_ID),
            ("GITHUB_RUN_ATTEMPT", ATTEMPT),
            ("GITHUB_JOB", JOB_ID),
        ],
        None,
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outputs = fs::read_to_string(output_file)?;
    assert!(outputs.contains("ready=false"), "{outputs}");
    assert!(
        outputs.contains("acceptance=cache_unavailable"),
        "{outputs}"
    );
    Ok(())
}
