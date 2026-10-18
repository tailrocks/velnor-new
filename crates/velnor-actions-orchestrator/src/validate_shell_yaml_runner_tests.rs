//! Hosted runner shell classification follows the canonical target authority.
use super::{ShellDialect, scan_workflow};

#[test]
fn every_supported_macos_label_resolves_the_hosted_bash_default() {
    for label in [
        "macos-14",
        "macos-15",
        "macos-15-arm64",
        "macos-26",
        "xcode-27",
        "macos-15-intel",
        "macos-26-intel",
    ] {
        let yaml = format!(
            "jobs:\n  probe:\n    runs-on: {label}\n    steps:\n      - name: probe\n        run: echo probe\n"
        );
        let runs = scan_workflow(&yaml).expect("supported macOS runner shell");
        assert_eq!(runs.len(), 1, "{label}");
        assert_eq!(runs[0].shell, ShellDialect::Bash, "{label}");
    }
}

#[test]
fn unsupported_and_dynamic_runner_labels_do_not_imply_bash() {
    for label in [
        "macos-unknown",
        "macos-latest",
        "xcode-unknown",
        "${{ matrix.runner }}",
    ] {
        let yaml = format!(
            "jobs:\n  probe:\n    runs-on: {label}\n    steps:\n      - name: probe\n        run: echo probe\n"
        );
        assert!(
            scan_workflow(&yaml)
                .is_err_and(|error| error.to_string().contains("shell_unresolved_for_runner")),
            "{label}"
        );
    }
}
