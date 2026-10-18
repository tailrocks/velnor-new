use super::*;

pub(super) fn assert_image_producer(body: &str) -> TestResult {
    assert!(body.contains("bash images/resource-probe/build-runner-images.sh"));
    assert!(body.contains("{{.Architecture}}"));
    assert!(body.contains("= amd64"));
    assert!(body.contains("docker save"));
    assert!(body.contains("sha256sum"));
    assert!(
        body.contains("actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8")
    );
    assert!(body.contains("gh release create"));
    assert!(body.contains("readonly tag_prefix='runner'"));
    assert!(body.contains("${tag_prefix}-${release_source_sha}"));
    assert!(body.contains(HOSTED_RUNS));
    assert_release_permissions(body, "attest-images", "publish-images")?;
    assert!(!body.contains("echo release"));
    assert!(body.contains("workflow_call:"));
    assert!(!body.contains("workflow_dispatch:"));
    assert!(!body.contains("packages:"));
    assert!(!body.contains("CARGO_REGISTRY_TOKEN"));
    assert!(!body.contains("secrets:"));
    assert!(!body.contains("pull_request"));
    run_image_build_fixture()?;
    Ok(())
}

fn run_image_build_fixture() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("The workspace root is missing.")?;
    let fixture = tempfile::tempdir()?;
    let bin = fixture.path().join("bin");
    let target = fixture.path().join("target");
    std::fs::create_dir_all(&bin)?;
    let probe = target.join("x86_64-unknown-linux-musl/release/velnor-resource-probe");
    std::fs::create_dir_all(
        probe
            .parent()
            .ok_or("The parent directory for the probe is missing.")?,
    )?;
    std::fs::write(&probe, b"source 0123456789012345678901234567890123456789")?;

    let file_mock = bin.join("file");
    std::fs::write(
        &file_mock,
        "#!/bin/sh\nprintf '%s\\n' 'ELF 64-bit LSB pie executable, x86-64, static-pie linked'\n",
    )?;
    let docker_mock = bin.join("docker");
    std::fs::write(
        &docker_mock,
        [
            "#!/bin/bash",
            "printf '<%s>' \"$@\" >> \"$DOCKER_CALLS\"",
            "printf '\\n' >> \"$DOCKER_CALLS\"",
            "previous=''",
            "for argument in \"$@\"; do",
            "  if [[ \"$previous\" == '--format' && \"$argument\" == '{{.Architecture}}' ]]; then printf 'amd64\\n'; fi",
            "  if [[ \"$previous\" == '--format' && \"$argument\" == '{{.Id}}' ]]; then printf 'sha256:%064d\\n' 0; fi",
            "  if [[ \"$previous\" == '--entrypoint' && \"$argument\" == '/usr/bin/uname' ]]; then printf 'x86_64\\n'; fi",
            "  if [[ \"$previous\" == '--entrypoint' && \"$argument\" == '/usr/bin/sha256sum' ]]; then printf '%s  /velnor/resource-probe\\n' \"$EXPECTED_SHA\"; fi",
            "  previous=\"$argument\"",
            "done",
            "",
        ]
        .join("\n"),
    )?;
    for path in [&file_mock, &docker_mock] {
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
    }
    let expected_sha = Command::new("shasum")
        .args(["-a", "256"])
        .arg(&probe)
        .output()?;
    assert!(expected_sha.status.success());
    let expected_sha = String::from_utf8(expected_sha.stdout)?
        .split_whitespace()
        .next()
        .ok_or("sha256 output is empty")?
        .to_owned();
    let old_path = std::env::var("PATH")?;
    let output = Command::new("bash")
        .arg(workspace.join("images/resource-probe/build-runner-images.sh"))
        .current_dir(workspace)
        .env("PATH", format!("{}:{old_path}", bin.display()))
        .env("CARGO_TARGET_DIR", &target)
        .env(
            "VELNOR_SOURCE_REVISION",
            "0123456789012345678901234567890123456789",
        )
        .env("RUNNER_TEMP", fixture.path())
        .env("DOCKER_CALLS", fixture.path().join("docker-calls"))
        .env("EXPECTED_SHA", expected_sha)
        .output()?;
    assert!(
        output.status.success(),
        "status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_docker_calls(&fixture.path().join("docker-calls"))
}

fn assert_docker_calls(path: &std::path::Path) -> TestResult {
    let calls = std::fs::read_to_string(path)?;
    assert_eq!(calls.matches("<buildx><build>").count(), 2, "{calls}");
    assert!(calls.contains("<--platform><linux/amd64>"), "{calls}");
    assert!(calls.contains("<resource_probe="), "{calls}");
    assert!(calls.contains("<images/runner/ubuntu-26.04>"), "{calls}");
    assert!(calls.contains("<images/dind>"), "{calls}");
    assert_eq!(
        calls.matches("<--entrypoint></usr/bin/uname>").count(),
        2,
        "{calls}"
    );
    assert_eq!(
        calls.matches("<--entrypoint></usr/bin/sha256sum>").count(),
        2,
        "{calls}"
    );
    Ok(())
}

pub(super) fn assert_macos_producer(body: &str) -> TestResult {
    let build = job_body(body, "build-binary")?;
    assert!(build.contains("runs-on: macos-15"), "{build}");
    assert!(build.contains("jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7"));
    assert!(
        build.contains(&format!(
            "rust@{}",
            velnor_actions_mise::catalog::RUST_VERSION
        )),
        "{build}"
    );
    assert!(
        build.contains(&format!(
            "mise --no-config --no-env --no-hooks exec rust@{} mr-boxington@{} -- mbx build --locked --manifest-path crates/velnor-runner/Cargo.toml --release --package velnor-runner-cli --bin velnor-host",
            velnor_actions_mise::catalog::RUST_VERSION,
            velnor_actions_mise::catalog::MR_BOXINGTON_VERSION
        )),
        "{build}"
    );
    assert!(body.contains("Mach-O"));
    assert!(body.contains("arm64"));
    assert!(body.contains("shasum -a 256"));
    assert!(
        body.contains("actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8")
    );
    assert!(body.contains("gh release create"));
    assert!(body.contains("readonly tag_prefix='binary'"));
    assert!(body.contains("${tag_prefix}-${release_source_sha}"));
    assert_release_permissions(body, "attest-binary", "publish-binary")?;
    assert!(!body.contains("echo release"));
    assert!(body.contains("workflow_call:"));
    assert!(!body.contains("workflow_dispatch:"));
    assert!(!body.contains("packages:"));
    assert!(!body.contains("CARGO_REGISTRY_TOKEN"));
    Ok(())
}

fn assert_release_permissions(body: &str, attest: &str, publish: &str) -> TestResult {
    assert!(body.contains("workflow_call:"), "{body}");
    assert!(!body.contains("workflow_dispatch:"), "{body}");
    let attest_body = job_body(body, attest)?;
    let publish_body = job_body(body, publish)?;
    assert!(attest_body.contains("id-token: write"), "{attest_body}");
    assert!(!attest_body.contains("contents: write"), "{attest_body}");
    assert!(!attest_body.contains("packages:"), "{attest_body}");
    assert!(publish_body.contains("contents: write"), "{publish_body}");
    assert!(
        publish_body.contains("GH_TOKEN: ${{ github.token }}"),
        "{publish_body}"
    );
    assert!(publish_body.contains("actions/checkout@"), "{publish_body}");
    assert!(
        publish_body.contains("release_repository='tailrocks/velnor-new'"),
        "{publish_body}"
    );
    assert!(!attest_body.contains("GH_TOKEN"), "{attest_body}");
    assert!(!publish_body.contains("id-token:"), "{publish_body}");
    Ok(())
}
