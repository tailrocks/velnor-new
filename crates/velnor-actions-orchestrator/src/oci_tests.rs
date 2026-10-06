use super::{
    OCI_DELIVERY_TREE_PATHS, OCI_WORKFLOW_PATH, OciRenderContext, render_oci_release,
    render_oci_support_files,
};
use std::collections::BTreeMap;
use velnor_actions_contract::config::{
    OciImage, OciPlatform, OciReleaseConfig, RegistryAuthentication,
};

pub(super) fn context() -> OciRenderContext {
    OciRenderContext {
        repository: "ChainArgos/blockchain-nodes".to_owned(),
        default_branch: "main".to_owned(),
        ci_workflow: "ci.yml".to_owned(),
        generator_version: "0.1.0".to_owned(),
        pins: super::super::delivery_pins::oci_pins(),
    }
}

pub(super) fn config() -> OciReleaseConfig {
    let images = ["base", "build", "heimdall"]
        .into_iter()
        .enumerate()
        .map(|(index, id)| OciImage {
            id: id.to_owned(),
            image: format!("chainargos/{id}"),
            context: format!("docker-{id}"),
            dockerfile: format!("docker-{id}/Dockerfile"),
            platforms: vec![OciPlatform::Amd64, OciPlatform::Arm64],
            lfs: id == "heimdall",
            depends_on: if index == 0 {
                Vec::new()
            } else {
                vec!["base".to_owned()]
            },
            build_args: BTreeMap::from([("FOO".to_owned(), "literal".to_owned())]),
        })
        .collect();
    OciReleaseConfig {
        enabled: true,
        registry: "docker.io".to_owned(),
        authentication: RegistryAuthentication::NamedSecrets {
            username_secret: "DOCKERHUB_USERNAME".to_owned(),
            password_secret: "DOCKERHUB_TOKEN".to_owned(),
        },
        images,
    }
}

#[test]
fn native_graph_remains_closed_until_owned_host_qualification() {
    let error =
        super::build_oci_workflow_ir(&config(), &context()).expect_err("owned host proof absent");
    assert!(error.to_string().contains("qualified distribution absent"));
}

#[test]
fn native_actions_bind_exact_subject_and_isolated_registry_path() {
    use velnor_actions_contract::StepKind;
    let ctx = context();
    let policy = config();
    let image = &policy.images[0];
    let attest = super::steps::attest_index(&policy, &ctx, image);
    let StepKind::Action { with, .. } = attest.kind else {
        panic!("pinned action")
    };
    assert_eq!(with["subject-name"], "docker.io/chainargos/base");
    assert_eq!(
        with["subject-digest"],
        "${{ steps.receipt.outputs.index_digest }}"
    );
    assert_eq!(with["push-to-registry"], "false");
    let login = super::steps::login(&policy, &ctx).expect("closed registry login");
    let StepKind::Action { env, .. } = login.kind else {
        panic!("pinned action")
    };
    assert_eq!(env["DOCKER_CONFIG"], "${{ runner.temp }}/velnor/oci-docker");
}

#[test]
fn floating_tools_and_dependency_cycles_fail() {
    let mut ctx = context();
    ctx.pins.buildx_version = "latest".to_owned();
    assert!(render_oci_release(&config(), &ctx).is_err());
    ctx = context();
    ctx.pins.buildkit_image = "moby/buildkit:latest".to_owned();
    assert!(render_oci_release(&config(), &ctx).is_err());
    let mut policy = config();
    policy.images[0].depends_on.push("heimdall".to_owned());
    assert!(render_oci_release(&policy, &context()).is_err());
}

#[test]
fn invalid_registry_authentication_is_rejected_before_sdk_admission() {
    let mut policy = config();
    policy.authentication = RegistryAuthentication::GithubToken;
    assert!(render_oci_release(&policy, &context()).is_err());
}

#[test]
fn fixed_support_inventory_embeds_immutable_module_map() {
    let mut paths = render_oci_support_files("0.1.0")
        .expect("support scripts")
        .into_iter()
        .map(|file| {
            velnor_actions_workflow_renderer::marker::check_first_line(&file.bytes, "0.1.0")
                .expect("source marker");
            file.path
        })
        .collect::<std::collections::BTreeSet<_>>();
    paths.insert(OCI_WORKFLOW_PATH.to_owned());
    assert_eq!(
        paths,
        OCI_DELIVERY_TREE_PATHS
            .iter()
            .map(|path| (*path).to_owned())
            .collect()
    );
    let files = render_oci_support_files("0.1.0").expect("compiled source");
    let wrapper = files
        .iter()
        .find(|file| file.path == velnor_actions_contract::SourceBoundOperation::OciDelivery.path())
        .expect("compiled wrapper");
    assert!(wrapper.bytes.contains("oci_delivery"));
    assert!(!wrapper.bytes.contains("${{"));
}
