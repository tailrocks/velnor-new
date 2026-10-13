//! The installed binary must report its compiled source identity exactly.
use std::process::Command;

#[test]
fn both_version_flags_report_the_compiled_release() {
    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_mbx"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("mbx {}\n", mbx::version::VERSION)
        );
    }
}
