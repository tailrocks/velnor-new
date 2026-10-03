use super::*;
use std::ffi::OsString;
use std::io::{Cursor, Read};

fn artifact(fresh: bool) -> serde_json::Value {
    serde_json::json!({"reason":"compiler-artifact","package_id":"path+file:///pkg#pkg@1.0.0",
        "manifest_path":"/pkg/Cargo.toml", "target":{"name":"pkg","kind":["lib"],
        "crate_types":["lib"],"src_path":"/pkg/src/lib.rs","edition":"2024","test":true},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,
        "overflow_checks":true,"test":false},"features":["feature"],
        "filenames":["/pkg/target/libpkg.rlib"],"executable":null,"fresh":fresh})
}

fn stream(lines: &[serde_json::Value]) -> Vec<u8> {
    lines
        .iter()
        .flat_map(|line| {
            let mut bytes = serde_json::to_vec(line).expect("fixture JSON");
            bytes.push(b'\n');
            bytes
        })
        .collect()
}

#[cfg(unix)]
fn capture(bytes: &[u8]) -> CargoCaptureReport {
    let binding = CargoCommandBinding::fixture(&["build", "--message-format=json"]);
    let mut output = Vec::new();
    let stdout = CargoStdoutCapture::read_stream(Cursor::new(bytes), &mut output, binding.clone());
    assert_eq!(output, bytes);
    let stderr = CargoStderrCapture::read_stream(Cursor::new([]), &mut Vec::new(), binding.clone());
    stdout.finish(CargoCommandCompletion::fixture(binding, true), stderr)
}

#[cfg(unix)]
#[test]
fn full_protocol_preserves_observations_without_execution_inference() {
    let bytes = stream(&[
        artifact(false),
        serde_json::json!({"reason":"build-script-executed",
        "package_id":"path+file:///pkg#pkg@1.0.0","linked_libs":["static=native"],
        "linked_paths":["native=/pkg/native"],"cfgs":["custom"],"env":[["KEY","VALUE"]],
        "out_dir":"/pkg/target/out"}),
        serde_json::json!({"reason":"build-finished","success":true}),
    ]);
    let report = capture(&bytes);
    assert!(report.protocol_complete);
    assert!(report.native_process_capture_complete);
    assert!(!report.public_json_unit_identity_available);
    assert!(report.package_source_authority.is_none());
    let CargoMessage::CompilerArtifact(artifact) = &report.messages[0] else {
        panic!("artifact fixture");
    };
    assert!(!artifact.fresh);
    assert_eq!(artifact.features, ["feature"]);
    assert_eq!(artifact.profile.debuginfo, CargoDebuginfo::Level2);
    assert_eq!(artifact.target.additional["test"], true);
    assert!(matches!(
        report.messages[1],
        CargoMessage::BuildScriptExecuted(_)
    ));
}

#[cfg(unix)]
#[test]
fn absent_truncated_malformed_and_conflicting_records_do_not_qualify() {
    assert!(!capture(b"").protocol_complete);
    assert!(!capture(br#"{"reason":"build-finished","success":true}"#).protocol_complete);
    assert!(!capture(b"\xff\n").protocol_complete);
    let duplicate = stream(&[
        artifact(true),
        artifact(false),
        serde_json::json!({"reason":"build-finished","success":true}),
    ]);
    let report = capture(&duplicate);
    assert!(!report.protocol_complete);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.detail.contains("conflicting"))
    );
    let duplicate_finish = stream(&[
        serde_json::json!({"reason":"build-finished","success":true}),
        serde_json::json!({"reason":"build-finished","success":true}),
    ]);
    assert!(!capture(&duplicate_finish).protocol_complete);
}

#[test]
fn native_selectors_cannot_be_supplied_as_other_option_values() {
    let classify =
        |args: &[&str]| cargo_frontend(&args.iter().map(OsString::from).collect::<Vec<_>>());
    assert_eq!(
        classify(&["build", "--config", "--message-format=json"]),
        CargoFrontend::Unsupported
    );
    assert_eq!(
        classify(&["test", "--message-format=json", "--config", "--no-run"]),
        CargoFrontend::Unsupported
    );
    assert_eq!(
        classify(&["test", "--message-format=json", "--no-run"]),
        CargoFrontend::TestPreparation
    );
    assert_eq!(
        classify(&["run", "--message-format=json"]),
        CargoFrontend::Unsupported
    );
    assert_eq!(
        classify(&["test", "--message-format=json", "--", "--no-run"]),
        CargoFrontend::Unsupported
    );
}

struct ReadFailure;
impl Read for ReadFailure {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("fixture read failed"))
    }
}

#[cfg(unix)]
#[test]
fn failed_stderr_read_and_foreign_completion_block_closure() {
    let binding = CargoCommandBinding::fixture(&["build", "--message-format=json"]);
    let bytes = stream(&[serde_json::json!({"reason":"build-finished","success":true})]);
    let stdout =
        CargoStdoutCapture::read_stream(Cursor::new(&bytes), &mut Vec::new(), binding.clone());
    let stderr = CargoStderrCapture::read_stream(ReadFailure, &mut Vec::new(), binding.clone());
    assert!(
        !stdout
            .finish(
                CargoCommandCompletion::fixture(binding.clone(), true),
                stderr
            )
            .native_process_capture_complete
    );
    let stdout =
        CargoStdoutCapture::read_stream(Cursor::new(&bytes), &mut Vec::new(), binding.clone());
    let foreign = CargoCommandBinding::fixture(&["check", "--message-format=json"]);
    let stderr = CargoStderrCapture::read_stream(Cursor::new([]), &mut Vec::new(), foreign.clone());
    assert!(
        !stdout
            .finish(CargoCommandCompletion::fixture(foreign, true), stderr)
            .protocol_complete
    );
}

#[cfg(unix)]
#[test]
fn budgets_bound_evidence_but_always_drain_and_forward_stdout() {
    let bytes = vec![b'x'; MAX_CAPTURE_BYTES as usize + 8192];
    let report = capture(&bytes);
    assert!(report.stdout_eof);
    assert!(report.capture_truncated);
    assert_eq!(report.stdout_bytes, bytes.len() as u64);
    assert!(!report.protocol_complete);
}

#[cfg(unix)]
#[test]
fn unsupported_commands_observe_raw_stream_without_false_parse_errors() {
    let binding = CargoCommandBinding::fixture(&["run"]);
    let stdout = CargoStdoutCapture::read_stream(
        Cursor::new(b"program output\n"),
        &mut Vec::new(),
        binding.clone(),
    );
    let stderr = CargoStderrCapture::read_stream(
        Cursor::new(b"diagnostic\n"),
        &mut Vec::new(),
        binding.clone(),
    );
    let report = stdout.finish(CargoCommandCompletion::fixture(binding, true), stderr);
    assert!(report.native_process_capture_complete);
    assert!(!report.protocol_complete);
    assert!(report.diagnostics.is_empty());
    assert_eq!(report.stderr.bytes(), b"diagnostic\n");
}

#[cfg(unix)]
#[test]
fn public_serialization_never_exports_raw_diagnostics_environment_or_extensions() {
    let secret = "PRIVATE_SECRET_SENTINEL";
    let mut observed_artifact = artifact(false);
    observed_artifact["extra"] = secret.into();
    observed_artifact["target"]["extra"] = secret.into();
    observed_artifact["profile"]["extra"] = secret.into();
    let bytes = stream(&[
        observed_artifact.clone(),
        serde_json::json!({"reason":"build-script-executed",
        "package_id":"pkg","linked_libs":[],"linked_paths":[],"cfgs":[],
        "env":[[secret,secret]],"out_dir":"/pkg/out","extra":secret}),
        serde_json::json!({"reason":"compiler-message","package_id":"pkg",
        "manifest_path":"/pkg/Cargo.toml","target":observed_artifact["target"],
        "message":{"rendered":secret,"message":secret},"extra":secret}),
        serde_json::json!({"reason":"build-finished","success":true,"extra":secret}),
    ]);
    let binding =
        CargoCommandBinding::fixture(&["build", "--message-format=json", "--config", secret]);
    let stdout =
        CargoStdoutCapture::read_stream(Cursor::new(&bytes), &mut Vec::new(), binding.clone());
    let stderr = CargoStderrCapture::read_stream(
        Cursor::new(secret.as_bytes()),
        &mut Vec::new(),
        binding.clone(),
    );
    let report = stdout.finish(CargoCommandCompletion::fixture(binding, true), stderr);
    let published = serde_json::to_string(&report).expect("public report serialization");
    assert!(!published.contains(secret), "private task data leaked");
    assert_eq!(report.build_script_environment_entries, 1);
    assert_eq!(report.stderr.bytes(), secret.as_bytes());
}

#[cfg(unix)]
#[test]
fn malformed_profile_values_never_qualify_or_escape_through_public_fields() {
    for invalid in [
        serde_json::json!({"PRIVATE_SECRET_SENTINEL":true}),
        serde_json::json!("PRIVATE_SECRET_SENTINEL"),
        serde_json::json!(3),
    ] {
        let mut observed = artifact(true);
        observed["profile"]["debuginfo"] = invalid;
        let report = capture(&stream(&[
            observed,
            serde_json::json!({"reason":"build-finished","success":true}),
        ]));
        assert!(!report.protocol_complete);
        assert!(
            !serde_json::to_string(&report)
                .expect("report")
                .contains("PRIVATE_SECRET_SENTINEL")
        );
    }
    for valid in [
        serde_json::json!(null),
        serde_json::json!(0),
        serde_json::json!(1),
        serde_json::json!(2),
        serde_json::json!("line-directives-only"),
        serde_json::json!("line-tables-only"),
    ] {
        let mut observed = artifact(true);
        observed["profile"]["debuginfo"] = valid;
        assert!(parse_cargo_message(&serde_json::to_vec(&observed).expect("fixture")).is_ok());
    }
}

#[cfg(unix)]
#[test]
fn real_spawn_binds_streams_terminal_status_and_exact_forwarded_bytes() {
    use std::process::{Command, Stdio};
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "printf native-stdout; printf native-stderr >&2"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (mut child, binding) =
        CargoCommandBinding::spawn(&mut command, "session".into(), "root".into())
            .expect("fixture spawn");
    let stdout = binding.take_stdout(&mut child).expect("bound stdout");
    let stderr = binding.take_stderr(&mut child).expect("bound stderr");
    let stdout_reader = std::thread::spawn(move || {
        let mut forwarded = Vec::new();
        let capture = CargoStdoutCapture::read(stdout, &mut forwarded);
        (capture, forwarded)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut forwarded = Vec::new();
        let capture = CargoStderrCapture::read(stderr, &mut forwarded);
        (capture, forwarded)
    });
    let completion = CargoCommandCompletion::wait(&mut child, binding).expect("actual wait");
    assert!(completion.status().success());
    assert!(completion.workload_wall_ns() > 0);
    let (stdout, stdout_bytes) = stdout_reader.join().expect("stdout reader");
    let (stderr, stderr_bytes) = stderr_reader.join().expect("stderr reader");
    assert_eq!(stdout_bytes, b"native-stdout");
    assert_eq!(stderr_bytes, b"native-stderr");
    let report = stdout.finish(completion, stderr);
    assert!(report.native_process_capture_complete);
    assert!(!report.protocol_complete);
    assert!(report.local_capture_complete());
    assert!(report.identity_matches("session", "root"));
    assert!(!report.identity_matches("other", "root"));
}

#[cfg(unix)]
#[test]
fn original_native_proof_rejects_public_and_retained_private_payload_mutation() {
    let bytes = stream(&[
        artifact(true),
        serde_json::json!({"reason":"build-finished","success":true}),
    ]);
    let mut public_mutated = capture(&bytes);
    assert!(public_mutated.local_capture_complete());
    assert!(public_mutated.identity_matches("test-session", "test-root"));
    public_mutated.stdout_bytes += 1;
    assert!(!public_mutated.local_capture_complete());
    assert!(!public_mutated.identity_matches("test-session", "test-root"));
    let mut private_mutated = capture(&bytes);
    let CargoMessage::CompilerArtifact(artifact) = &mut private_mutated.messages[0] else {
        panic!("fixture artifact");
    };
    artifact
        .additional
        .insert("unpublished-extension".into(), true.into());
    assert!(!private_mutated.local_capture_complete());
    let mut forged_complete = capture(b"");
    // Actual empty streams prove native closure; unsupported protocol never
    // becomes protocol evidence by changing its published status.
    assert!(forged_complete.local_capture_complete());
    forged_complete.protocol_complete = true;
    assert!(!forged_complete.local_capture_complete());
}
