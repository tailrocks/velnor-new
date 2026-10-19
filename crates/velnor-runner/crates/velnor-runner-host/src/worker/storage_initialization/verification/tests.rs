use std::collections::HashMap;

use super::{
    capabilities_match, engine_identity_matches, image_environment_matches, volume_identity_matches,
};

#[test]
fn image_environment_allows_only_the_pinned_base_path() {
    assert!(image_environment_matches(&[
        super::IMAGE_PATH_ENV.to_owned()
    ]));
    assert!(!image_environment_matches(&[]));
    assert!(!image_environment_matches(&[
        super::IMAGE_PATH_ENV.to_owned(),
        "TOKEN=secret".to_owned()
    ]));
    assert!(!image_environment_matches(&[
        super::IMAGE_PATH_ENV.to_owned(),
        super::IMAGE_PATH_ENV.to_owned()
    ]));
}

#[test]
fn capability_inspection_accepts_known_prefix_and_order() {
    assert!(capabilities_match(
        &["CAP_FOWNER".to_owned(), "CAP_CHOWN".to_owned()],
        &["CHOWN", "FOWNER"]
    ));
    assert!(capabilities_match(
        &["FOWNER".to_owned(), "CHOWN".to_owned()],
        &["CHOWN", "FOWNER"]
    ));
}

#[test]
fn capability_inspection_rejects_duplicates_unknowns_and_extras() {
    assert!(!capabilities_match(
        &["CAP_CHOWN".to_owned(), "CAP_CHOWN".to_owned()],
        &["CHOWN", "FOWNER"]
    ));
    assert!(!capabilities_match(
        &["CAP_CHOWN".to_owned(), "CAP_SYS_ADMIN".to_owned()],
        &["CHOWN", "FOWNER"]
    ));
    assert!(!capabilities_match(
        &[
            "CAP_CHOWN".to_owned(),
            "CAP_FOWNER".to_owned(),
            "CAP_NET_ADMIN".to_owned()
        ],
        &["CHOWN", "FOWNER"]
    ));
}

#[test]
fn engine_identity_requires_the_selected_daemon_and_root() {
    assert!(engine_identity_matches(
        "engine-a", "root-a", "engine-a", "root-a"
    ));
    assert!(!engine_identity_matches(
        "engine-b", "root-a", "engine-a", "root-a"
    ));
    assert!(!engine_identity_matches(
        "engine-a", "root-b", "engine-a", "root-a"
    ));
    assert!(!engine_identity_matches("", "root-a", "engine-a", "root-a"));
}

#[test]
fn volume_identity_requires_name_worker_and_role() {
    let worker = "w0123456789abcdef0123456789abcdef";
    let name = format!("{worker}-home");
    let labels = HashMap::from([
        ("velnor.worker".to_owned(), worker.to_owned()),
        ("velnor.role".to_owned(), "home".to_owned()),
    ]);
    assert!(volume_identity_matches(
        &name, &labels, &name, worker, "home"
    ));
    assert!(!volume_identity_matches(
        "foreign-home",
        &labels,
        &name,
        worker,
        "home"
    ));
    assert!(!volume_identity_matches(
        &name, &labels, &name, "foreign", "home"
    ));
    assert!(!volume_identity_matches(
        &name, &labels, &name, worker, "work"
    ));
    assert!(!volume_identity_matches(
        &name,
        &HashMap::new(),
        &name,
        worker,
        "home"
    ));
}
