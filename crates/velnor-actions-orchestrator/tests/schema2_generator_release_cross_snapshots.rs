use super::action_snapshots::{Actions, action};
use velnor_actions_workflow_renderer::setup::{
    MISE_BINARY_SHA256_MACOS_ARM64, MISE_BINARY_SHA256_MACOS_X64,
};

/// The Intel leg builds on ARM runners and qualifies natively on Intel.
pub(super) fn assert_intel_cross_build(
    body: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    let intel = super::super::job_body(body, "build-macos-intel")?;
    assert!(intel.contains("runs-on: macos-15\n"), "{intel}");
    let intel_action = action(actions, "generator-release-build-macos-intel")?;
    assert!(intel_action.contains("*Mach-O*x86_64*"), "{intel_action}");
    assert!(intel_action.contains("shasum -a 256"), "{intel_action}");
    assert!(
        intel_action.contains("--target x86_64-apple-darwin"),
        "{intel_action}"
    );
    assert!(
        intel_action.contains("target/x86_64-apple-darwin/release/velnor-actions"),
        "{intel_action}"
    );
    assert!(
        !intel_action.contains("cp target/release/velnor-actions"),
        "{intel_action}"
    );
    assert!(
        intel_action.contains(
            "rustup target add --toolchain 1.98.1-aarch64-apple-darwin x86_64-apple-darwin"
        ),
        "{intel_action}"
    );
    assert!(
        intel_action.contains(&format!("sha256: {MISE_BINARY_SHA256_MACOS_ARM64}")),
        "{intel_action}"
    );
    assert!(
        !intel_action.contains(MISE_BINARY_SHA256_MACOS_X64),
        "{intel_action}"
    );
    let qualify = super::super::job_body(body, "qualify-macos-intel")?;
    assert!(qualify.contains("runs-on: macos-15-intel\n"), "{qualify}");
    Ok(())
}
