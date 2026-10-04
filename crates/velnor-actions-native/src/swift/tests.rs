#[test]
fn native_helper_adversarial_tests() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/swift");
    let status = std::process::Command::new("python3")
        .args([
            "-B",
            "-m",
            "unittest",
            "desktop_native_test",
            "desktop_native_security_test",
            "desktop_native_sign_test",
            "desktop_native_state_test",
        ])
        .current_dir(directory)
        .status()?;
    assert!(status.success(), "native Swift adversarial tests failed");
    Ok(())
}
