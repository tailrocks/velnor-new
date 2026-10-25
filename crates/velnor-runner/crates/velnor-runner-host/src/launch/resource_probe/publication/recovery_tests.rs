use crate::launch::resource_probe::projection::DockerRoot;

use super::recover::fresh_identity_matches;

#[test]
fn recovery_requires_the_current_engine_and_docker_root() {
    let root = DockerRoot::parse("/var/lib/docker").expect("valid Docker root");
    let digest = root.digest();

    assert!(fresh_identity_matches(
        Some("selected-engine"),
        Some("/var/lib/docker"),
        "selected-engine",
        digest,
    ));
    assert!(!fresh_identity_matches(
        Some("other-engine"),
        Some("/var/lib/docker"),
        "selected-engine",
        digest,
    ));
    assert!(!fresh_identity_matches(
        Some("selected-engine"),
        Some("/var/lib/other"),
        "selected-engine",
        digest,
    ));
    assert!(!fresh_identity_matches(
        Some("selected-engine"),
        None,
        "selected-engine",
        digest,
    ));
}
