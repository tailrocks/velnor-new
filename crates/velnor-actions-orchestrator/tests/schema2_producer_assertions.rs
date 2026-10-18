use super::*;

pub(super) fn assert_image_producer(body: &str) -> TestResult {
    assert!(body.contains("docker build --platform linux/amd64"));
    assert!(body.contains("images/runner/ubuntu-26.04"));
    assert!(body.contains("images/dind"));
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
