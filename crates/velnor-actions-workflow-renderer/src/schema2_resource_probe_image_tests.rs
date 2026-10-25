use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::{ExecutionMode, RoutingWorkflow};

use super::product_release_family::{self, Family};
use super::product_release_test_pins::test_pins;
use super::{Schema2WorkflowRequest, product_release, resource_probe_image};
use crate::yaml::{Yaml, render_yaml};

const IMAGE_BUILD_SCRIPT: &str =
    include_str!("../../../images/resource-probe/build-runner-images.sh");
const PROBE_BUILD_SCRIPT: &str = include_str!("../../../images/resource-probe/build-image.sh");

#[test]
fn image_family_uses_pinned_musl_build_and_exact_five_file_inventory() -> Result<(), Box<dyn Error>>
{
    let pins = test_pins();
    let steps = resource_probe_image::build_steps(&pins)?;
    let step_text = render_yaml(&Yaml::Seq(steps));
    for expected in [
        "Install pinned resource-probe build and qualification tools",
        "Install pinned Linux musl target",
        "rustup target add --toolchain",
        "x86_64-unknown-linux-musl",
        "Build locked resource probe",
        "mbx build --locked --manifest-path crates/velnor-runner/Cargo.toml",
        "--package velnor-resource-probe --bin velnor-resource-probe --release --target x86_64-unknown-linux-musl",
        "bash images/resource-probe/build-image.sh",
        "Qualify native resource-probe producer and shared archive fixtures",
        "mbx nextest run --locked --manifest-path crates/velnor-runner/Cargo.toml",
        "--package velnor-runner-host --lib --target x86_64-unknown-linux-gnu",
        "--bin velnor-resource-probe-release --release --target x86_64-unknown-linux-gnu",
        "VELNOR_RESOURCE_PROBE_RELEASE_BIN",
    ] {
        assert!(
            step_text.contains(expected),
            "missing {expected}: {step_text}"
        );
    }

    let expected_assets = [
        "velnor-runner-linux-amd64.tar",
        "velnor-dind-linux-amd64.tar",
        "velnor-resource-probe-linux-amd64.tar",
        "RESOURCE_PROBE_MANIFEST.json",
        "SHA256SUMS",
    ];
    let pins = test_pins();
    let prepare = product_release_family::prepare_script(Family::Images, &pins)?;
    let publish = product_release_family::publish_script(Family::Images, &pins)?;
    for asset in expected_assets {
        assert!(prepare.contains(asset), "prepare omits {asset}");
        assert!(publish.contains(asset), "publish omits {asset}");
    }
    assert_eq!(
        product_release_family::Family::Images.asset_names(),
        expected_assets.map(str::to_owned)
    );

    let scale_set = Schema2WorkflowRequest::canonical_scale_set()?;
    let request = Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: scale_set.clone(),
        routing: crate::workflow_routing::test_policy(ExecutionMode::Hosted, scale_set),
        workflows: BTreeSet::from([RoutingWorkflow::ImageRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(test_pins()),
    };
    let product = product_release::render(&request)?.ok_or("image workflow was not rendered")?;
    let workflow = product
        .family_workflows
        .iter()
        .find(|(path, _)| path == Family::Images.workflow_path())
        .ok_or("image family workflow is missing")?;
    let rendered = render_yaml(&workflow.1);
    for asset in expected_assets {
        assert!(rendered.contains(asset), "image workflow omits {asset}");
    }
    assert_image_save_preserves_validated_probe(&rendered);
    let attest_job = field(field(&workflow.1, "jobs")?, "attest-images")?;
    let steps = sequence(field(attest_job, "steps")?)?;
    let attest = steps
        .iter()
        .find(|step| {
            field(step, "name").is_ok_and(|name| name == &Yaml::str("Attest built artifacts"))
        })
        .ok_or("attestation step is missing")?;
    let Yaml::Str(subject) = field(field(attest, "with")?, "subject-path")? else {
        return Err("attestation subject path is not text".into());
    };
    let attested_assets = subject
        .lines()
        .map(|line| line.strip_prefix("assets/").unwrap_or("invalid"))
        .collect::<Vec<_>>();
    assert_eq!(attested_assets, expected_assets.to_vec());
    Ok(())
}

fn assert_image_save_preserves_validated_probe(rendered: &str) {
    let source_binding = rendered
        .find("Bind resource probe to the release source")
        .expect("probe source revision is bound before compilation");
    let probe_build = rendered
        .find("Build locked resource probe")
        .expect("probe compile step is present");
    let probe_image = rendered
        .find("Build and smoke-test resource-probe image")
        .expect("probe image is built before the runner images");
    let runner_images = rendered
        .find("bash images/resource-probe/build-runner-images.sh")
        .expect("Runner and DinD use the shared static probe context");
    assert!(
        source_binding < probe_build,
        "Set the source revision before compile."
    );
    assert!(
        probe_build < probe_image,
        "Build the probe executable first."
    );
    assert!(
        probe_image < runner_images,
        "Runner and DinD use the checked probe"
    );
    assert!(rendered.contains("VELNOR_SOURCE_REVISION=%s"));
    assert!(rendered.contains("missing source revision"));
    assert!(
        rendered.contains("docker save --output velnor-runner-linux-amd64.tar"),
        "runner archive remains produced by the existing image-save step"
    );
    assert!(
        rendered.contains("docker save --output velnor-dind-linux-amd64.tar"),
        "DinD archive remains produced by the existing image-save step"
    );
    assert!(
        !rendered.contains("docker save --output velnor-resource-probe-linux-amd64.tar"),
        "the validated probe archive must not be overwritten"
    );
    assert!(
        rendered.contains("test -s velnor-resource-probe-linux-amd64.tar"),
        "the later save step must verify the already-produced probe archive"
    );
    assert!(IMAGE_BUILD_SCRIPT.contains("--build-context \"resource_probe=$context\""));
    assert!(IMAGE_BUILD_SCRIPT.contains("resource probe does not contain its source revision"));
    assert!(IMAGE_BUILD_SCRIPT.contains("--entrypoint /usr/bin/sha256sum"));
    assert!(IMAGE_BUILD_SCRIPT.contains("[[ \"$actual_sha256\" == \"$expected_sha256\" ]]"));
    assert!(PROBE_BUILD_SCRIPT.contains("source_revision=\"${VELNOR_SOURCE_REVISION:?"));
    assert!(PROBE_BUILD_SCRIPT.contains("[[ \"$source_revision\" == \"$source_sha\" ]]"));
    assert!(
        PROBE_BUILD_SCRIPT.contains("validate-smoke-schema5 \"$smoke_output\" \"$smoke_root\"")
    );
    assert!(PROBE_BUILD_SCRIPT.contains(
        "python3 -B images/resource-probe/validate_smoke.py \"$smoke_output\" \"$smoke_root\""
    ));
    assert!(PROBE_BUILD_SCRIPT.contains("bash images/resource-probe/test_container_lifecycle.sh"));
    assert!(PROBE_BUILD_SCRIPT.contains("--cgroupns=host --pid=host"));
    assert!(PROBE_BUILD_SCRIPT.contains("--memory=134217728 --memory-swap=134217728"));
    assert!(
        PROBE_BUILD_SCRIPT
            .contains("--mount \"type=bind,source=/sys/fs/cgroup,target=/sys/fs/cgroup,readonly\"")
    );
}

fn field<'a>(value: &'a Yaml, key: &str) -> Result<&'a Yaml, Box<dyn Error>> {
    let Yaml::Map(fields) = value else {
        return Err(format!("expected mapping while reading {key}").into());
    };
    fields
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .ok_or_else(|| format!("missing mapping field {key}").into())
}

fn sequence(value: &Yaml) -> Result<&[Yaml], Box<dyn Error>> {
    match value {
        Yaml::Seq(items) => Ok(items),
        _ => Err("expected sequence".into()),
    }
}
