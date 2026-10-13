use super::*;
use crate::yaml::render_yaml;

fn run_step<'a>(job: &'a Yaml, step_name: &str) -> &'a str {
    let Yaml::Map(job_fields) = job else {
        panic!("job must be a mapping");
    };
    let Some((_, Yaml::Seq(steps))) = job_fields.iter().find(|(key, _)| key == "steps") else {
        panic!("job steps are missing");
    };
    let step = steps
        .iter()
        .find(|step| {
            let Yaml::Map(fields) = step else {
                return false;
            };
            fields
                .iter()
                .any(|(key, value)| key == "name" && value == &Yaml::str(step_name))
        })
        .unwrap_or_else(|| panic!("step {step_name} is missing"));
    let Yaml::Map(fields) = step else {
        unreachable!("selected step is a mapping");
    };
    let Some((_, Yaml::Str(run))) = fields.iter().find(|(key, _)| key == "run") else {
        panic!("step {step_name} has no string run value");
    };
    run
}

fn pins() -> RustToolchainQualificationPins {
    RustToolchainQualificationPins {
        mise_setup: crate::setup::MiseSetup {
            uses: format!("jdx/mise-action@{}", "a".repeat(40)),
            version: velnor_actions_mise::MISE_VERSION.to_owned(),
            sha256: crate::setup::MISE_BINARY_SHA256_LINUX_X64.to_owned(),
        },
        mbx_version: velnor_actions_mise::MR_BOXINGTON_VERSION.to_owned(),
        rust_version: "1.99.0".to_owned(),
        manifest_url: "https://static.rust-lang.org/dist/channel-rust-1.99.0.toml".to_owned(),
        manifest_sha256: MANIFEST_SHA256.to_owned(),
    }
}

#[test]
fn qualification_measures_both_hosted_targets_without_claiming_a_result() {
    let jobs = jobs(&pins()).expect("catalog-aligned official pins render");
    assert_eq!(
        jobs.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        ["rust-toolchain-linux-x64", "rust-toolchain-macos-arm64"]
    );

    for (index, (id, body)) in jobs.iter().enumerate() {
        assert_hosted_target_setup(index, id, body);
        assert_measurement_artifact(index, id, body);
    }
}

fn assert_hosted_target_setup(index: usize, id: &str, body: &Yaml) {
    let rendered = render_yaml(body);
    let runner = if index == 0 {
        "ubuntu-26.04"
    } else {
        "macos-26"
    };
    assert!(rendered.contains(&format!("runs-on: {runner}")), "{id}");
    assert!(
        rendered.contains("if: inputs.mode == 'rust-toolchain'"),
        "{id}"
    );
    assert!(rendered.contains("permissions:\n  contents: read"), "{id}");
    assert!(!rendered.contains("actions: write"), "{id}");
    assert!(rendered.contains("ref: ${{ github.sha }}"), "{id}");
    assert!(rendered.contains("persist-credentials: \"false\""), "{id}");
    assert!(rendered.contains("name: Setup Mise"), "{id}");
    assert!(rendered.contains("version: 2026.10.7"), "{id}");
    let mise_sha = if index == 0 {
        crate::setup::MISE_BINARY_SHA256_LINUX_X64
    } else {
        crate::setup::MISE_BINARY_SHA256_MACOS_ARM64
    };
    assert!(rendered.contains(mise_sha), "{id}");
    assert!(rendered.contains("MBX_VERSION: 1.23.0"), "{id}");
    assert_pinned_mbx_install(id, body);
}

fn assert_pinned_mbx_install(id: &str, body: &Yaml) {
    let install_run = run_step(body, "Install pinned MBX through Mise");
    assert!(
        install_run
            .contains("mise --no-config --no-env --no-hooks install \"mr-boxington@$MBX_VERSION\""),
        "{id}: {install_run:?}"
    );
    assert!(
        install_run.contains(
            "mise --no-config --no-env --no-hooks which mbx --tool \"mr-boxington@$MBX_VERSION\""
        ),
        "{id}: {install_run:?}"
    );
}

fn assert_measurement_artifact(index: usize, id: &str, body: &Yaml) {
    let rendered = render_yaml(body);
    let (platform, artifact) = if index == 0 {
        ("linux_x64", "rust-toolchain-linux-x64")
    } else {
        ("macos_arm64", "rust-toolchain-macos-arm64")
    };
    assert!(rendered.contains("RUST_VERSION: 1.99.0"), "{id}");
    assert!(
        rendered.contains(&format!("QUALIFICATION_PLATFORM: {platform}")),
        "{id}"
    );
    assert!(rendered.contains(MANIFEST_SHA256), "{id}");
    assert!(
        rendered.contains("name: Install official Rust components and measure the qualified tree"),
        "{id}"
    );
    assert_qualification_probe(body, id);
    assert!(
        rendered.contains("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"),
        "{id}"
    );
    assert!(rendered.contains(&format!("name: {artifact}")), "{id}");
    assert!(rendered.contains("if-no-files-found: error"), "{id}");
    assert!(!rendered.contains("secrets."), "{id}");
    assert!(!rendered.contains("publish"), "{id}");
}

fn assert_qualification_probe(body: &Yaml, id: &str) {
    let probe_run = run_step(
        body,
        "Install official Rust components and measure the qualified tree",
    );
    assert!(
        probe_run.contains("--project-root \"$GITHUB_WORKSPACE\""),
        "{id}: {probe_run:?}"
    );
    assert!(
        probe_run.contains("--mbx-executable \"$MBX_EXECUTABLE\""),
        "{id}: {probe_run:?}"
    );
    assert!(
        probe_run.contains("--mbx-version \"$MBX_VERSION\""),
        "{id}: {probe_run:?}"
    );
    assert!(
        probe_run.contains("python3 scripts/qualification/qualify_rust_toolchain.py"),
        "{id}: {probe_run:?}"
    );
}

#[test]
fn qualification_rejects_unreviewed_version_url_and_digest() {
    let mut candidate = pins();
    candidate.rust_version = "1.100.0".to_owned();
    assert!(jobs(&candidate).is_err());

    let mut candidate = pins();
    candidate.manifest_url = "https://example.invalid/channel.toml".to_owned();
    assert!(jobs(&candidate).is_err());

    let mut candidate = pins();
    candidate.manifest_sha256 = "a".repeat(64);
    assert!(jobs(&candidate).is_err());

    let mut candidate = pins();
    candidate.mbx_version = "1.23.1".to_owned();
    assert!(jobs(&candidate).is_err());
}
