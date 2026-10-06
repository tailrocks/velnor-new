use super::*;

pub(super) fn config() -> AptDeliveryConfig {
    AptDeliveryConfig {
        source_repository: "tailrocks/velnor".to_owned(),
        consumer_repository: "tailrocks/velnor-apt".to_owned(),
        package: "velnor-runner".to_owned(),
        binary: "velnor-runner".to_owned(),
        identity_directory: "velnor".to_owned(),
        manifest_schema: "velnor.package-release.v1".to_owned(),
        keyring: "velnor.gpg".to_owned(),
        signer_fingerprint: "7E66E3A53F9B3B5CA61D0F53261EDAC957DEB801".to_owned(),
        origin: "Velnor".to_owned(),
        description: "apt repository for Velnor".to_owned(),
        feed_url: "https://velnor-apt.tailrocks.com".to_owned(),
        branch: "main".to_owned(),
        schedule: "17 4 * * *".to_owned(),
        signer_workflow: ".github/workflows/ci-release-package-signer.yml".to_owned(),
        oci_image_repository: "ghcr.io/tailrocks/velnor-job-ubuntu".to_owned(),
        oci_signer_workflow: ".github/workflows/delivery-oci.yml".to_owned(),
    }
}

pub(super) fn context() -> AptRenderContext {
    AptRenderContext {
        workflow: crate::apt_delivery::AptWorkflowContext {
            generator_version: "0.1.0".to_owned(),
            runs_on: "ubuntu-24.04".to_owned(),
        },
        tools: velnor_actions_workflow_renderer::delivery_tools::DeliveryToolContext {
            mise: crate::test_mise::setup("2026.10.0", &"b".repeat(64)),
            python_version: velnor_actions_mise::ToolCatalog::pinned()
                .version(velnor_actions_mise::PinnedTool::Python)
                .to_owned(),
            gh_version: velnor_actions_mise::ToolCatalog::pinned()
                .version(velnor_actions_mise::PinnedTool::Gh)
                .to_owned(),
            preparation: source_control_preparation(),
        },
        buildx_action: format!("docker/setup-buildx-action@{}", "c".repeat(40)),
        buildx_version: "v0.37.2".to_owned(),
        buildkit_image: format!("moby/buildkit@sha256:{}", "d".repeat(64)),
    }
}

fn source_control_preparation() -> velnor_actions_contract::CompiledSourceHelper {
    use velnor_actions_mise::catalog::{delivery_tools, qualification::DistributionHost};
    let host = DistributionHost::LinuxAmd64;
    let tools = delivery_tools::source_intent_control_tools(host, "0.1.0")
        .expect("SDK-owned Python source-control preparation");
    delivery_tools::validate_source_intent_control_tools(host, "0.1.0", &tools)
        .expect("exact source-control owner binding");
    tools.preparation().clone()
}

#[test]
fn closed_files_are_deterministic_and_marked() {
    let first = render_apt_delivery(&config(), &context()).expect("render");
    assert_eq!(
        first,
        render_apt_delivery(&config(), &context()).expect("rerender")
    );
    assert!(first.windows(2).all(|pair| pair[0].path < pair[1].path));
    assert!(
        first
            .iter()
            .any(|file| file.path == APT_DELIVERY_WORKFLOW_PATH)
    );
    assert!(
        first
            .iter()
            .any(|file| file.path.ends_with("apt-delivery.jsonc"))
    );
    for file in first {
        marker::check_first_line(&file.bytes, "0.1.0").expect("marker");
        assert!(!file.bytes.contains("velnor-workflow"));
        assert!(!file.bytes.contains("shell=True"));
    }
}

#[test]
fn malformed_policy_cannot_reach_fixed_renderer() {
    let mut policy = config();
    policy.source_repository = "${{ secrets.TOKEN }}".to_owned();
    assert!(render_apt_delivery(&policy, &context()).is_err());
    policy = config();
    policy.feed_url = "https://example.com/../../outside".to_owned();
    assert!(render_apt_delivery(&policy, &context()).is_err());
}
