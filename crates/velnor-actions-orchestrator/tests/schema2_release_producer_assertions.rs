use super::{HOSTED_RUNS, TestResult, job_body};

const PROBE_BUILD: &str = include_str!("../../../images/resource-probe/build-image.sh");
const RUNNER_IMAGE_BUILD: &str =
    include_str!("../../../images/resource-probe/build-runner-images.sh");

pub(super) fn assert_image_producer(body: &str) -> TestResult {
    // Keep the workflow bound to both reviewed source-build helpers.
    assert!(body.contains("bash images/resource-probe/build-image.sh"));
    assert!(body.contains("bash images/resource-probe/build-runner-images.sh"));
    let probe_compile = required_position(
        body,
        "Build locked resource probe",
        "resource-probe compile step is present",
    )?;
    let probe_smoke = required_position(
        body,
        "Build and smoke-test resource-probe image",
        "resource-probe smoke step is present",
    )?;
    let runner_build = required_position(
        body,
        "Build Runner and DinD images",
        "Runner and DinD build step is present",
    )?;
    assert!(probe_compile < probe_smoke && probe_smoke < runner_build);
    assert_probe_build_source()?;
    assert_runner_image_build_source()?;
    assert!(body.contains("{{.Architecture}}"));
    assert!(body.contains("= amd64"));
    assert!(body.contains("docker save"));
    assert!(body.contains("sha256sum"));
    assert!(body.contains("velnor-resource-probe-linux-amd64.tar"));
    assert!(body.contains("RESOURCE_PROBE_MANIFEST.json"));
    assert!(
        body.contains("actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8")
    );
    assert!(RUNNER_IMAGE_BUILD.contains("images/runner/ubuntu-26.04"));
    assert!(RUNNER_IMAGE_BUILD.contains("images/dind"));
    assert!(RUNNER_IMAGE_BUILD.contains("docker buildx build"));
    assert!(RUNNER_IMAGE_BUILD.contains("--platform linux/amd64"));
    assert!(RUNNER_IMAGE_BUILD.contains("docker image inspect --format '{{.Architecture}}'"));
    assert!(RUNNER_IMAGE_BUILD.contains("[[ \"$architecture\" == amd64 ]]"));
    assert!(body.contains("test \\\"$runner\\\" = amd64"));
    assert!(body.contains("test \\\"$dind\\\" = amd64"));
    assert!(body.contains("docker save"));
    assert!(body.contains("sha256sum"));
    assert!(body.contains("velnor-resource-probe-linux-amd64.tar"));
    assert!(body.contains("RESOURCE_PROBE_MANIFEST.json"));
    assert!(body.contains("gh release create"));
    assert!(body.contains("readonly tag_prefix='runner'"));
    assert!(body.contains("${tag_prefix}-${release_source_sha}"));
    assert!(body.contains(HOSTED_RUNS));
    assert_release_permissions(body, "attest-images", "publish-images")?;
    super::schema2_product_release_snapshots::assert_release_workflow_verifier(
        body,
        "Verify Rust channel manifest",
        "CARGO_REGISTRY_TOKEN",
        "Install pinned resource-probe build and qualification tools",
        Some(("GITHUB_TOKEN", "${{ github.token }}")),
    )?;
    assert!(!body.contains("echo release"));
    assert!(body.contains("workflow_call:"));
    assert!(!body.contains("workflow_dispatch:"));
    assert!(!body.contains("packages:"));
    assert!(body.contains("CARGO_REGISTRY_TOKEN: \"\""));
    assert!(!body.contains("secrets:"));
    assert!(!body.contains("pull_request"));
    Ok(())
}

fn assert_probe_build_source() -> TestResult {
    assert!(PROBE_BUILD.contains("docker build --platform linux/amd64 --network none"));
    assert!(PROBE_BUILD.contains("[[ \"$source_revision\" == \"$source_sha\" ]]"));
    assert!(PROBE_BUILD.contains("resource probe does not contain its source revision"));
    assert!(PROBE_BUILD.contains("python3 -B images/resource-probe/test_write_manifest.py"));
    assert!(PROBE_BUILD.contains("python3 -B images/resource-probe/test_validate_smoke.py"));
    let native_schema5_validation = required_position(
        PROBE_BUILD,
        "\"$release_validator\" validate-smoke-schema5 \"$smoke_output\" \"$smoke_root\"",
        "native resource-probe schema-5 validation is present",
    )?;
    let python_schema5_validation = required_position(
        PROBE_BUILD,
        "python3 -B images/resource-probe/validate_smoke.py \"$smoke_output\" \"$smoke_root\"",
        "Python resource-probe schema-5 validation is present",
    )?;
    assert!(native_schema5_validation < python_schema5_validation);
    assert!(PROBE_BUILD.contains("--network none --read-only --cap-drop=ALL"));
    let source_check = required_position(
        PROBE_BUILD,
        "[[ \"$source_revision\" == \"$source_sha\" ]]",
        "resource-probe source revision check is present",
    )?;
    let probe_build = required_position(
        PROBE_BUILD,
        "docker build --platform linux/amd64 --network none",
        "resource-probe build is present",
    )?;
    assert!(source_check < probe_build);
    Ok(())
}

fn assert_runner_image_build_source() -> TestResult {
    assert!(RUNNER_IMAGE_BUILD.contains("images/runner/ubuntu-26.04"));
    assert!(RUNNER_IMAGE_BUILD.contains("images/dind"));
    assert!(RUNNER_IMAGE_BUILD.contains("docker buildx build"));
    assert!(RUNNER_IMAGE_BUILD.contains("--platform linux/amd64"));
    assert!(RUNNER_IMAGE_BUILD.contains("docker image inspect --format '{{.Architecture}}'"));
    assert!(RUNNER_IMAGE_BUILD.contains("[[ \"$architecture\" == amd64 ]]"));
    assert!(RUNNER_IMAGE_BUILD.contains("--network none"));
    assert!(RUNNER_IMAGE_BUILD.contains("--read-only"));
    assert!(RUNNER_IMAGE_BUILD.contains("--cap-drop=ALL"));
    assert!(RUNNER_IMAGE_BUILD.contains("--security-opt no-new-privileges:true"));
    let source_check = required_position(
        RUNNER_IMAGE_BUILD,
        "[[ \"$checked_out_revision\" == \"$source_revision\" ]]",
        "runner source revision check is present",
    )?;
    let runner_compile = required_position(
        RUNNER_IMAGE_BUILD,
        "mise --no-config --no-env --no-hooks exec rust@1.99.1 -- cargo build",
        "runner helper compile step is present",
    )?;
    assert!(source_check < runner_compile);
    Ok(())
}

fn required_position(
    content: &str,
    needle: &str,
    message: &'static str,
) -> Result<usize, std::io::Error> {
    content
        .find(needle)
        .ok_or_else(|| std::io::Error::other(message))
}

pub(super) fn assert_macos_producer(body: &str) -> TestResult {
    let build = job_body(body, "build-binary")?;
    assert!(build.contains("runs-on: macos-15"), "{build}");
    assert!(build.contains("jdx/mise-action@82a612c5ab8bcda7cc5db6b7e81a8e7584c3dc6e"));
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
    super::schema2_product_release_snapshots::assert_release_workflow_verifier(
        body,
        "Verify Rust channel manifest",
        "CARGO_REGISTRY_TOKEN",
        "Install pinned Rust and MBX",
        None,
    )?;
    assert!(!body.contains("echo release"));
    assert!(body.contains("workflow_call:"));
    assert!(!body.contains("workflow_dispatch:"));
    assert!(!body.contains("packages:"));
    assert!(build.contains("CARGO_REGISTRY_TOKEN: \"\""));
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
