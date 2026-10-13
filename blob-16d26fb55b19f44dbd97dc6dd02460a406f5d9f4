//! CLI shell integration cases: the `velnor-actions` binary surface exists.
#[test]
fn version_flag_reports_binary_name() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
        .arg("--version")
        .output()
        .expect("spawn velnor-actions --version");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("velnor-actions "));
}
