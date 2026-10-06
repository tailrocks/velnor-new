//! Native publication tools bound to the qualified compiled action catalog.
use crate::delivery_emit::oci_delivery::{OciActionPins, OciRenderContext};
use crate::prepare::GenerationPreparation;
use velnor_actions_actionlint::actions::{
    ATTEST_ACTION_SHA, BUILD_PUSH_ACTION_SHA, BUILDKIT_IMAGE_DIGEST, BUILDX_VERSION,
    CHECKOUT_ACTION_SHA, LOGIN_ACTION_SHA, SBOM_SCANNER_IMAGE_DIGEST, SETUP_BUILDX_ACTION_SHA,
    UPLOAD_ARTIFACT_ACTION_SHA,
};

/// Exact official setup action source revision.
pub(crate) fn buildx_action() -> String {
    action("docker/setup-buildx-action", SETUP_BUILDX_ACTION_SHA)
}
/// Exact binary release, distinct from setup action source.
pub(crate) fn buildx_version() -> String {
    format!("v{BUILDX_VERSION}")
}
/// Immutable multi-platform image for the qualified BuildKit release.
pub(crate) fn buildkit_image() -> String {
    format!("moby/buildkit@sha256:{BUILDKIT_IMAGE_DIGEST}")
}
/// Platform helper tools and publication identities from reviewed catalogs.
pub(super) fn oci_context(prep: &GenerationPreparation, repository: String) -> OciRenderContext {
    OciRenderContext {
        repository,
        default_branch: prep.default_branch.clone(),
        ci_workflow: "ci.yml".to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        pins: oci_pins(),
    }
}

/// Exact reviewed official OCI action and runtime identities.
pub(super) fn oci_pins() -> OciActionPins {
    OciActionPins {
        checkout: action("actions/checkout", CHECKOUT_ACTION_SHA),
        upload: action("actions/upload-artifact", UPLOAD_ARTIFACT_ACTION_SHA),
        buildx: buildx_action(),
        login: action("docker/login-action", LOGIN_ACTION_SHA),
        build: action("docker/build-push-action", BUILD_PUSH_ACTION_SHA),
        attest: action("actions/attest", ATTEST_ACTION_SHA),
        buildx_version: buildx_version(),
        buildkit_image: buildkit_image(),
        sbom_image: format!("docker/buildkit-syft-scanner@sha256:{SBOM_SCANNER_IMAGE_DIGEST}"),
    }
}

/// An approved action name and its complete immutable source revision.
fn action(repository: &str, sha: &str) -> String {
    format!("{repository}@{sha}")
}
