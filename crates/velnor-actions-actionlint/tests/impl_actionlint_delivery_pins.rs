//! Qualified delivery pins accept exact references and reject mutable references.
use velnor_actions_actionlint::actions::{
    ATTEST_ACTION_SHA, ATTEST_ACTION_VERSION, BUILD_PUSH_ACTION_SHA, BUILD_PUSH_ACTION_VERSION,
    CONFIGURE_PAGES_ACTION_SHA, CONFIGURE_PAGES_ACTION_VERSION, DEPLOY_PAGES_ACTION_SHA,
    DEPLOY_PAGES_ACTION_VERSION, LOGIN_ACTION_SHA, LOGIN_ACTION_VERSION, SETUP_BUILDX_ACTION_SHA,
    SETUP_BUILDX_ACTION_VERSION, UPLOAD_PAGES_ARTIFACT_ACTION_SHA,
    UPLOAD_PAGES_ARTIFACT_ACTION_VERSION,
};
use velnor_actions_actionlint::{ActionlintError, PinnedActionRef};

#[test]
fn delivery_constants_match_qualified_official_records() {
    assert_eq!(SETUP_BUILDX_ACTION_VERSION, "v4.4.1");
    assert_eq!(
        SETUP_BUILDX_ACTION_SHA,
        "f87e5991a6d7451dcb8d9637bfbc97413f497069"
    );
    assert_eq!(LOGIN_ACTION_VERSION, "v4.6.0");
    assert_eq!(LOGIN_ACTION_SHA, "dbcb813823bdd20940b903addbd779551569679f");
    assert_eq!(BUILD_PUSH_ACTION_VERSION, "v7.4.0");
    assert_eq!(
        BUILD_PUSH_ACTION_SHA,
        "c3c9e263c25d99ce0380d002d59b67737d91b0dc"
    );
    assert_eq!(CONFIGURE_PAGES_ACTION_VERSION, "v6.0.0");
    assert_eq!(
        CONFIGURE_PAGES_ACTION_SHA,
        "45bfe0192ca1faeb007ade9deae92b16b8254a0d"
    );
    assert_eq!(UPLOAD_PAGES_ARTIFACT_ACTION_VERSION, "v5.0.0");
    assert_eq!(
        UPLOAD_PAGES_ARTIFACT_ACTION_SHA,
        "fc324d3547104276b827a68afc52ff2a11cc49c9"
    );
    assert_eq!(DEPLOY_PAGES_ACTION_VERSION, "v5.0.1");
    assert_eq!(
        DEPLOY_PAGES_ACTION_SHA,
        "368f82528645a54fb793d4d04e342629a3f51346"
    );
    assert_eq!(ATTEST_ACTION_VERSION, "v4.2.2");
    assert_eq!(
        ATTEST_ACTION_SHA,
        "1e69f48acb82d1966a394da916b4c1698aa569d6"
    );
}

#[test]
fn delivery_refs_accept_full_sha_and_reject_moving_refs() {
    for (key, sha, version) in [
        (
            "docker/setup-buildx-action",
            SETUP_BUILDX_ACTION_SHA,
            SETUP_BUILDX_ACTION_VERSION,
        ),
        (
            "docker/login-action",
            LOGIN_ACTION_SHA,
            LOGIN_ACTION_VERSION,
        ),
        (
            "docker/build-push-action",
            BUILD_PUSH_ACTION_SHA,
            BUILD_PUSH_ACTION_VERSION,
        ),
        (
            "actions/configure-pages",
            CONFIGURE_PAGES_ACTION_SHA,
            CONFIGURE_PAGES_ACTION_VERSION,
        ),
        (
            "actions/upload-pages-artifact",
            UPLOAD_PAGES_ARTIFACT_ACTION_SHA,
            UPLOAD_PAGES_ARTIFACT_ACTION_VERSION,
        ),
        (
            "actions/deploy-pages",
            DEPLOY_PAGES_ACTION_SHA,
            DEPLOY_PAGES_ACTION_VERSION,
        ),
        ("actions/attest", ATTEST_ACTION_SHA, ATTEST_ACTION_VERSION),
    ] {
        let exact = format!("{key}@{sha}");
        let reference = PinnedActionRef::parse_uses(&exact, version);
        assert!(reference.is_ok(), "qualified pin rejected: {exact}");
        for moving in [version, "main", &sha[..7]] {
            assert!(
                matches!(
                    PinnedActionRef::parse_uses(&format!("{key}@{moving}"), version),
                    Err(ActionlintError::InvalidPin { .. })
                ),
                "moving ref accepted: {key}@{moving}"
            );
        }
    }
}
