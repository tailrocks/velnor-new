//! Candidate release-manifest rejection cases through the existing CLI path.

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn check_release_rejects_invalid_candidate_manifests() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let root = root.canonicalize()?;
    let harness = root.join("scripts/test-capture-opentofu-goldens-bin.sh");
    let output = Command::new("bash")
        .arg(harness)
        .arg(env!("CARGO_BIN_EXE_velnor-actions"))
        .current_dir(root)
        .output()?;

    assert!(
        output.status.success(),
        "release manifest harness failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
