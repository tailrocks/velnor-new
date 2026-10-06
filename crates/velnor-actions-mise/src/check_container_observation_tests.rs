//! Synthetic parsing fixtures; no actual app/signature qualification is claimed.
use super::{ContainerProbeOutput, OrbStackAppObservation};

#[test]
fn plist_executable_binds_declared_main_file_not_an_app_neighbor() -> Result<(), crate::MiseError> {
    let info = r#"{"CFBundleExecutable":"OrbStack"}"#;
    super::app::validate_main_executable(info, "Contents/MacOS/OrbStack")?;
    for declared in [
        "Contents/MacOS/Other",
        "Contents/Resources/OrbStack",
        "Contents/MacOS/OrbStack-copy",
    ] {
        assert!(super::app::validate_main_executable(info, declared).is_err());
    }
    for info in [
        "{}",
        r#"{"CFBundleExecutable":"../OrbStack"}"#,
        r#"{"CFBundleExecutable":""}"#,
    ] {
        assert!(super::app::validate_main_executable(info, "Contents/MacOS/OrbStack").is_err());
    }
    Ok(())
}

fn output(stdout: &str, stderr: &str) -> ContainerProbeOutput {
    ContainerProbeOutput {
        stdout: stdout.into(),
        stderr: stderr.into(),
        stdout_digest: velnor_actions_contract::digest_b3(stdout.as_bytes()),
        stderr_digest: velnor_actions_contract::digest_b3(stderr.as_bytes()),
    }
}
fn app(source: &str, owned: &str) -> Result<OrbStackAppObservation, crate::MiseError> {
    OrbStackAppObservation::parse(
        output(
            r#"{"CFBundleIdentifier":"dev.fixture.app","CFBundleShortVersionString":"2.2.3","CFBundleVersion":"20963"}"#,
            "",
        ),
        output(
            "",
            "Identifier=dev.fixture.app\nTeamIdentifier=HUAQ24HBR6\n",
        ),
        output("", ""),
        output("", ""),
        output("", ""),
        output("", source),
        output("", owned),
    )
}

#[test]
fn running_state_rejects_negated_partial_and_unknown_status() {
    assert!(super::orbstack_running("Running\n"));
    for status in [
        "not running",
        "Stopped",
        "Starting",
        "Running but unhealthy",
        "running",
        " Running",
        "",
    ] {
        assert!(!super::orbstack_running(status));
    }
}

#[test]
fn signed_app_and_nested_cli_require_exact_team_and_same_nested_identity()
-> Result<(), crate::MiseError> {
    let identity = "Identifier=dev.fixture.cli\nTeamIdentifier=HUAQ24HBR6\n";
    let observed = app(identity, identity)?;
    assert_eq!(observed.version, "2.2.3");
    assert_eq!(observed.build, "20963");
    assert_eq!(observed.team_id, "HUAQ24HBR6");
    for wrong in [
        "Identifier=dev.fixture.cli\nTeamIdentifier=OTHERTEAM1\n",
        "Identifier=dev.different.cli\nTeamIdentifier=HUAQ24HBR6\n",
        "Identifier=dev.fixture.cli\nTeamIdentifier=HUAQ24HBR6\nTeamIdentifier=HUAQ24HBR6\n",
        "Identifier=dev.fixture.cli\n",
    ] {
        assert!(app(identity, wrong).is_err());
        assert!(app(wrong, identity).is_err());
    }
    Ok(())
}

#[test]
fn captured_stdout_and_stderr_must_match_their_byte_digests() {
    let mut stream = output("{}", "signed identity");
    stream.stdout.push(' ');
    assert!(
        super::validate_streams(&stream, super::MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES)
            .is_err()
    );
    let mut stream = output("{}", "signed identity");
    stream.stderr.push(' ');
    assert!(
        super::validate_streams(&stream, super::MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES)
            .is_err()
    );
}

#[test]
fn container_probe_streams_respect_receipt_and_text_bounds() {
    let mut stream = output("{}", "");
    stream.stdout = "x".repeat(super::MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES + 1);
    stream.stdout_digest = velnor_actions_contract::digest_b3(stream.stdout.as_bytes());
    assert!(
        super::validate_streams(&stream, super::MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES).is_err()
    );
    let mut stream = output("{}", "");
    stream.stderr.push('\0');
    stream.stderr_digest = velnor_actions_contract::digest_b3(stream.stderr.as_bytes());
    assert!(
        super::validate_streams(&stream, super::MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES).is_err()
    );
}

#[test]
fn fixed_daemon_json_rejects_empty_id_duplicates_and_unknown_platform() {
    for json in [
        r#"{"ID":"","ServerVersion":"29.4.0","OSType":"linux","Architecture":"aarch64","OperatingSystem":"OrbStack"}"#,
        r#"{"ID":"id","ID":"id2","ServerVersion":"29.4.0","OSType":"linux","Architecture":"aarch64","OperatingSystem":"OrbStack"}"#,
        r#"{"ID":"id","ServerVersion":"29.4.0","OSType":"windows","Architecture":"aarch64","OperatingSystem":"OrbStack"}"#,
        r#"{"ID":"id","ServerVersion":"29.4.0","OSType":"linux","Architecture":"unknown","OperatingSystem":"OrbStack"}"#,
    ] {
        assert!(super::DockerDaemonObservation::parse(output(json, "")).is_err());
    }
}
