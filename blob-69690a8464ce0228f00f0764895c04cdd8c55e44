use super::*;
use crate::setup::MiseSetup;
use crate::yaml::render_yaml;
use std::process::{Command, Output};

const LINUX_OUTPUT: &str = "2026.10.6 linux-x64 (2026-10-09)";
const MACOS_OUTPUT: &str = "2026.10.6 macos-x64 (2026-10-09)";

fn pins() -> MisePinQualificationPins {
    MisePinQualificationPins {
        linux_x86_64_setup: MiseSetup {
            uses: format!("jdx/mise-action@{}", "a".repeat(40)),
            version: "2026.10.6".to_owned(),
            sha256: "b".repeat(64),
        },
        macos_x86_64_setup: MiseSetup {
            uses: format!("jdx/mise-action@{}", "a".repeat(40)),
            version: "2026.10.6".to_owned(),
            sha256: "c".repeat(64),
        },
    }
}

#[test]
fn jobs_are_opt_in_exact_ref_read_only_and_no_publication() {
    let jobs = jobs(&pins()).expect("valid pins render");
    assert_eq!(
        jobs.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        ["mise-pin-linux-x64", "mise-pin-macos-x64"]
    );

    for (id, body) in &jobs {
        let rendered = render_yaml(body);
        assert!(rendered.contains("if: inputs.mode == 'mise-pin'"), "{id}");
        assert!(rendered.contains("permissions:\n  contents: read"), "{id}");
        assert!(rendered.contains("ref: ${{ github.sha }}"), "{id}");
        assert!(rendered.contains("persist-credentials: \"false\""), "{id}");
        assert!(rendered.contains("version: 2026.10.6"), "{id}");
        assert!(rendered.contains("install: \"false\""), "{id}");
        assert!(rendered.contains("env: \"false\""), "{id}");
        assert!(rendered.contains("cache: \"false\""), "{id}");
        assert!(rendered.contains("cache_save: \"false\""), "{id}");
        assert!(rendered.contains("git rev-parse HEAD"), "{id}");
        assert!(rendered.contains("mise --version"), "{id}");
        assert!(rendered.contains("MISE_SHA256"), "{id}");
        assert!(!rendered.contains("secrets."), "{id}");
        assert!(!rendered.contains("actions: write"), "{id}");
        assert!(!rendered.contains("artifact"), "{id}");
        assert!(!rendered.contains("publish"), "{id}");
    }

    let linux = render_yaml(&jobs[0].1);
    let macos = render_yaml(&jobs[1].1);
    assert!(linux.contains("runs-on: ubuntu-26.04"));
    assert!(linux.contains(&format!("sha256: {}", "b".repeat(64))));
    assert!(linux.contains("sha256sum"));
    assert!(macos.contains("runs-on: macos-15-intel"));
    assert!(macos.contains(&format!("sha256: {}", "c".repeat(64))));
    assert!(macos.contains("shasum -a 256"));
}

#[test]
fn mismatched_candidate_versions_fail_closed() {
    let mut pins = pins();
    pins.macos_x86_64_setup.version = "2026.10.5".to_owned();
    assert!(jobs(&pins).is_err_and(|error| {
        error
            .to_string()
            .contains("mise_pin_qualification_version_mismatch")
    }));
}

#[test]
fn verification_accepts_native_x64_version_output_before_digest_check() {
    let digest = "b".repeat(64);

    for (target, version_output) in [
        (ReleaseTarget::LinuxX86_64, LINUX_OUTPUT),
        (ReleaseTarget::MacosX86_64, MACOS_OUTPUT),
    ] {
        let output = run_verification(target, version_output, &digest, &digest);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("DIGEST_CHECK_REACHED"),
            "version must pass before the digest command runs"
        );
    }
}

#[test]
fn verification_rejects_invalid_version_platform_and_legacy_output_before_digest() {
    let digest = "b".repeat(64);
    let cases = [
        (
            ReleaseTarget::LinuxX86_64,
            "2026.10.5 linux-x64 (2026-10-09)",
        ),
        (
            ReleaseTarget::LinuxX86_64,
            "2026.10.6 macos-x64 (2026-10-09)",
        ),
        (
            ReleaseTarget::LinuxX86_64,
            "mise 2026.10.6 linux-x64 (2026-10-09)",
        ),
        (ReleaseTarget::MacosX86_64, "2026.10.6 macos-x64 (2026-1-9)"),
    ];

    for (target, version_output) in cases {
        let output = run_verification(target, version_output, &digest, &digest);
        assert!(!output.status.success(), "{version_output}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("DIGEST_CHECK_REACHED"),
            "invalid version must fail before the digest command runs"
        );
    }
}

#[test]
fn verification_rejects_a_mismatched_binary_digest_after_version_check() {
    let expected = "b".repeat(64);
    let actual = "c".repeat(64);
    let output = run_verification(ReleaseTarget::LinuxX86_64, LINUX_OUTPUT, &expected, &actual);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("DIGEST_CHECK_REACHED"));
}

fn run_verification(
    target: ReleaseTarget,
    version_output: &str,
    expected_digest: &str,
    actual_digest: &str,
) -> Output {
    let script = format!(
        r#"git() {{ test "$1" = "rev-parse"; test "$2" = "HEAD"; printf '%s\n' "$GITHUB_SHA"; }}
mise() {{ printf '%s\n' "$MISE_OUTPUT"; }}
sha256sum() {{ printf 'DIGEST_CHECK_REACHED\n' >&2; printf '%s  %s\n' "$ACTUAL_DIGEST" "$1"; }}
shasum() {{ test "$1" = "-a"; test "$2" = "256"; printf 'DIGEST_CHECK_REACHED\n' >&2; printf '%s  %s\n' "$ACTUAL_DIGEST" "$3"; }}
{}"#,
        verification_script(target).expect("x64 target has a verification script")
    );

    Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("GITHUB_SHA", "selected-ref")
        .env("MISE_VERSION", "2026.10.6")
        .env("MISE_SHA256", expected_digest)
        .env("MISE_OUTPUT", version_output)
        .env("ACTUAL_DIGEST", actual_digest)
        .output()
        .expect("bash is available for hosted qualification tests")
}
