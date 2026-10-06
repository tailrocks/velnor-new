//! Regressions for the checkout-owned archive guard build cache boundary.

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[test]
fn shared_target_isolated_by_source_bytes_when_mtime_is_preserved() -> Result<(), Box<dyn Error>> {
    let output = Command::new("bash")
        .arg("scripts/test-build-owned-archive-guard.sh")
        .current_dir(workspace_root())
        .output()?;
    assert!(
        output.status.success(),
        "archive guard builder regression failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("archive guard target and bytecode regression passed")
    );
    Ok(())
}
