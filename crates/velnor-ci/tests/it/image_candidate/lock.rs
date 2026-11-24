// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_ci::image_candidate::build::{buildx_arguments, forwards};
use velnor_ci::image_candidate::lock::{Lock, banner, parse_lock, plugin};

fn locked() -> Lock {
    Lock {
        buildx_version: "0.38.0".to_owned(),
        buildx_sha256: "a".repeat(64),
    }
}

fn banner_verdict(text: &str) -> String {
    banner(text, &locked()).unwrap_or_else(|e| e.to_string())
}

#[test]
fn the_buildx_banner_must_name_the_locked_version_on_one_line() {
    assert_eq!(
        banner_verdict("github.com/docker/buildx v0.38.0 abc123\n"),
        "github.com/docker/buildx v0.38.0 abc123"
    );
    for refused in [
        "github.com/docker/buildx v0.38.1 abc",
        "github.com/docker/buildx v0.38.0 abc\nsecond line",
        "docker buildx 0.38.0",
        "  \n",
    ] {
        let verdict = banner_verdict(refused);
        assert!(
            verdict.starts_with("Docker Buildx"),
            "{refused:?}: {verdict}"
        );
    }
}

#[test]
fn exactly_one_buildx_plugin_with_a_path_is_required() {
    let one =
        r#"[{"Name":"compose","Path":"/c"},{"Name":"buildx","Path":"/b","Version":"v0.38.0"}]"#;
    let found = plugin(one).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found.path.to_string_lossy(), "/b");
    assert_eq!(found.version.as_deref(), Some("v0.38.0"));
    for refused in [
        "null",
        "[]",
        r#"[{"Name":"buildx","Path":"/a"},{"Name":"buildx","Path":"/b"}]"#,
        r#"[{"Name":"buildx"}]"#,
        "not json",
    ] {
        assert!(plugin(refused).is_err(), "{refused}");
    }
}

#[test]
fn the_runner_lock_must_pin_the_buildx_version_and_hash() {
    let parsed = parse_lock("buildx_version = \"0.38.0\"\nbuildx_sha256 = \"abc\"\nother = 1\n");
    assert_eq!(
        parsed.unwrap_or_else(|e| unreachable!("{e}")),
        Lock {
            buildx_version: "0.38.0".to_owned(),
            buildx_sha256: "abc".to_owned()
        }
    );
    assert!(parse_lock("buildx_version = \"0.38.0\"\n").is_err());
    assert!(parse_lock("buildx_sha256 = \"abc\"\n").is_err());
}

fn output_argument(archive: &str) -> String {
    let arguments = buildx_arguments(
        Path::new("/c/Dockerfile"),
        Path::new(archive),
        Path::new("/c"),
    );
    let at = arguments.iter().position(|argument| argument == "--output");
    at.and_then(|at| arguments.get(at + 1))
        .map_or_default(|value| value.to_string_lossy().into_owned())
}

#[test]
fn the_archive_path_is_one_csv_field_of_the_output_specification() {
    let tail = ",compression=gzip,force-compression=true";
    assert_eq!(
        output_argument("/out/a.tar"),
        format!("type=oci,dest=/out/a.tar{tail}")
    );
    assert_eq!(
        output_argument("/out,x/a\"b.tar"),
        format!("type=oci,\"dest=/out,x/a\"\"b.tar\"{tail}")
    );
}

#[test]
fn the_docker_environment_drops_secrets_and_daemon_selection() {
    for dropped in [
        "GH_TOKEN",
        "github_token",
        "REGISTRY_PASSWORD",
        "AWS_SECRET_ACCESS_KEY",
        "SSH_AUTH_SOCK",
        "GOOGLE_CREDENTIALS",
        "DOCKER_CONTEXT",
        "DOCKER_HOST",
        "BUILDX_BUILDER",
        "BUILDX_CONFIG",
        "BUILDKIT_HOST",
    ] {
        assert!(!forwards(dropped), "{dropped} was forwarded");
    }
    for kept in ["PATH", "HOME", "DOCKER_BUILDKIT", "BUILDKIT_PROGRESS"] {
        assert!(forwards(kept), "{kept} was dropped");
    }
}
