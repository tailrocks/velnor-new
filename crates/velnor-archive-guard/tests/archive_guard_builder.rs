//! Exercise the source-admitted native archive guard bootstrap.

use std::{error::Error, path::Path, process::Command};

const GUARD: &str = env!("CARGO_BIN_EXE_velnor-archive-guard");

#[test]
fn checkout_builder_uses_fresh_native_bootstrap_and_content_namespace() -> Result<(), Box<dyn Error>>
{
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root is missing")?;
    let output = Command::new("bash")
        .arg("scripts/test-build-owned-archive-guard.sh")
        .current_dir(workspace)
        .env("VELNOR_ARCHIVE_GUARD_TEST_NATIVE", GUARD)
        .output()?;
    assert!(
        output.status.success(),
        "archive guard builder regression failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("archive guard target and native bootstrap regression passed")
    );
    Ok(())
}
