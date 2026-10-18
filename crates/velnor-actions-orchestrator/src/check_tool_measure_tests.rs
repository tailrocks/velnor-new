use super::*;
use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use std::time::Duration;
use velnor_actions_contract::config::{QualifiedToolBackend, QualifiedToolOptions};

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(60)).expect("deadline")
}

fn archive_bytes() -> Vec<u8> {
    let payload = b"#!/bin/sh\nprintf 'node 24.1.0\\n'\n";
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_path("node-release/bin/node").expect("path");
    header.set_size(u64::try_from(payload.len()).expect("payload length fits u64"));
    header.set_mode(0o755);
    header.set_cksum();
    archive
        .append(&header, payload.as_slice())
        .expect("archive entry");
    archive
        .into_inner()
        .expect("tar stream")
        .finish()
        .expect("gzip stream")
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            use std::fmt::Write as _;
            write!(&mut output, "{byte:02x}").expect("write digest");
            output
        })
}

fn fixture_platform_for_host(os: &str, arch: &str) -> Option<CheckPlatform> {
    match (os, arch) {
        ("linux", "x86_64") => Some(CheckPlatform::LinuxX64),
        ("macos", "aarch64") => Some(CheckPlatform::MacosArm64),
        ("macos", "x86_64") => Some(CheckPlatform::MacosX64),
        _ => None,
    }
}

fn node_archive_filename(platform: CheckPlatform) -> &'static str {
    match platform {
        CheckPlatform::LinuxX64 => "node-v24.1.0-linux-x64.tar.gz",
        CheckPlatform::MacosArm64 => "node-v24.1.0-darwin-arm64.tar.gz",
        CheckPlatform::MacosX64 => "node-v24.1.0-darwin-x64.tar.gz",
    }
}

fn native_fixture_platform() -> CheckPlatform {
    fixture_platform_for_host(std::env::consts::OS, std::env::consts::ARCH)
        .expect("measurement fixture needs a supported host platform")
}

fn request(archive_path: &Path, archive_sha256: &str) -> ToolMeasurementRequest {
    let platform = native_fixture_platform();
    ToolMeasurementRequest {
        declaration: ToolMeasurementDeclaration {
            id: "node-measurement".into(),
            backend: QualifiedToolBackend::Core {
                tool: "node".into(),
            },
            version: "24.1.0".into(),
            options: QualifiedToolOptions::Default,
            depends_on: vec![],
            executables: vec![MeasurementExecutableDeclaration {
                name: "node".into(),
                path: "bin/node".into(),
                probe: QualifiedToolProbe::Version {
                    expected: "node 24.1.0".into(),
                },
            }],
        },
        platform,
        artifacts: vec![MeasurementArchiveInput {
            artifact: QualifiedToolArtifact {
                url: format!(
                    "https://nodejs.org/dist/v24.1.0/{}",
                    node_archive_filename(platform)
                ),
                sha256: archive_sha256.into(),
            },
            archive_path: archive_path.to_owned(),
        }],
        compiler_tool_id: None,
    }
}

#[test]
fn node_fixture_platform_table_covers_linux_x64_and_macos_arm64() {
    let cases = [
        (
            "linux",
            "x86_64",
            CheckPlatform::LinuxX64,
            "node-v24.1.0-linux-x64.tar.gz",
        ),
        (
            "macos",
            "aarch64",
            CheckPlatform::MacosArm64,
            "node-v24.1.0-darwin-arm64.tar.gz",
        ),
    ];

    for (os, arch, expected_platform, expected_archive) in cases {
        let platform = fixture_platform_for_host(os, arch).expect("table platform");
        assert_eq!(platform, expected_platform);
        assert_eq!(node_archive_filename(platform), expected_archive);
    }
}

fn error_problem(error: OrchestratorError) -> String {
    match error {
        OrchestratorError::Internal { problem } => problem,
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn wrong_source_archive_digest_is_rejected_before_extraction() {
    let fixture = tempfile::tempdir().expect("fixture");
    let bytes = archive_bytes();
    let archive = fixture.path().join("candidate.tar.gz");
    std::fs::write(&archive, &bytes).expect("write archive");
    let actual = sha256(&bytes);
    let wrong = actual.strip_prefix('a').map_or_else(
        || format!("a{}", &actual[1..]),
        |suffix| format!("b{suffix}"),
    );
    let request = request(&archive, &wrong);
    let home = tempfile::tempdir().expect("home");

    let error = measure_tool_candidate(&request, home.path(), deadline())
        .expect_err("wrong archive digest must fail closed");

    assert_eq!(error_problem(error), "qualified_tool_artifact_sha256");
    assert!(!home.path().join("tools/node-measurement/prefix").exists());
}

#[test]
fn unsupported_platform_fails_before_reading_or_creating_tool_state() {
    let fixture = tempfile::tempdir().expect("fixture");
    let mut request = request(&fixture.path().join("missing.tar.gz"), &"a".repeat(64));
    request.platform = match request.platform {
        CheckPlatform::LinuxX64 => CheckPlatform::MacosArm64,
        CheckPlatform::MacosArm64 | CheckPlatform::MacosX64 => CheckPlatform::LinuxX64,
    };
    let home = tempfile::tempdir().expect("home");

    let error = measure_tool_candidate(&request, home.path(), deadline())
        .expect_err("non-native measurement platform must fail closed");

    assert_eq!(
        error_problem(error),
        "qualified_tool_measurement_platform_unsupported"
    );
    assert!(!home.path().join("tools").exists());
}

#[test]
fn oversized_candidate_request_is_rejected_before_creating_tool_state() {
    let fixture = tempfile::tempdir().expect("fixture");
    let mut request = request(&fixture.path().join("missing.tar.gz"), &"a".repeat(64));
    let extra = request.declaration.executables[0].clone();
    request
        .declaration
        .executables
        .extend(std::iter::repeat_n(extra, MAX_MEASUREMENT_EXECUTABLES));
    let home = tempfile::tempdir().expect("home");

    let error = measure_tool_candidate(&request, home.path(), deadline())
        .expect_err("oversized request must fail closed");

    assert_eq!(
        error_problem(error),
        "qualified_tool_measurement_input_limit"
    );
    assert!(!home.path().join("tools").exists());
}

#[test]
fn successful_measurement_returns_untrusted_candidate_without_receipt_links_or_config_writes() {
    let fixture = tempfile::tempdir().expect("fixture");
    let bytes = archive_bytes();
    let archive = fixture.path().join("candidate.tar.gz");
    std::fs::write(&archive, &bytes).expect("write archive");
    let request = request(&archive, &sha256(&bytes));
    let home = tempfile::tempdir().expect("home");
    let repository = tempfile::tempdir().expect("repository");
    std::fs::create_dir(repository.path().join(".velnor")).expect("config dir");
    let config = repository.path().join(".velnor/config.toml");
    let config_before = b"qualified_tools = []\n";
    std::fs::write(&config, config_before).expect("config");

    let candidate = measure_tool_candidate(&request, home.path(), deadline()).expect("measurement");

    assert_eq!(
        candidate.schema,
        "velnor.qualified-tool-measurement-candidate.v1"
    );
    assert_eq!(candidate.tool_id, "node-measurement");
    assert_eq!(
        candidate.source_artifacts,
        [request.artifacts[0].artifact.clone()]
    );
    assert_eq!(candidate.executable_observations.len(), 1);
    assert_eq!(candidate.executable_observations[0].stdout, "node 24.1.0\n");
    assert_eq!(
        candidate.executable_observations[0].measured_sha256,
        sha256(b"#!/bin/sh\nprintf 'node 24.1.0\\n'\n")
    );
    assert!(!home.path().join("bin/node").exists());
    assert!(
        !home
            .path()
            .join("tools/node-measurement/probe-bin/node")
            .exists()
    );
    assert_eq!(std::fs::read(config).expect("read config"), config_before);

    let json = serde_json::to_value(candidate).expect("candidate JSON");
    assert!(json.get("definition_digest").is_none());
    assert!(json.get("qualified_tool_receipt").is_none());
    assert!(json.get("install_tree_sha256").is_none());
    assert!(json.get("measured_install_tree_sha256").is_some());
}
