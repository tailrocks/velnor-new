use std::collections::HashMap;

use super::{capabilities_match, engine_identity_matches, image_environment_matches};

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
fn volume_identity_accepts_created_home_labels() {
    let connection = "scoped-adapter";
    let reservation = 1;
    let operation = 1;
    let name = "w0123456789abcdef0123456789abcdef-home";
    let labels = created_home_labels(connection, reservation, operation);
    assert!(identity_matches(VolumeIdentityCase {
        actual_name: name,
        labels: &labels,
        expected_name: name,
        connection,
        reservation,
        operation,
        role: "runner-home",
    }));
}

#[test]
fn volume_identity_rejects_foreign_or_legacy_labels() {
    let connection = "scoped-adapter";
    let reservation = 1;
    let operation = 1;
    let name = "w0123456789abcdef0123456789abcdef-home";
    let labels = created_home_labels(connection, reservation, operation);
    let no_labels = HashMap::new();
    let old_labels = HashMap::from([
        (
            "velnor.worker".to_owned(),
            "w0123456789abcdef0123456789abcdef".to_owned(),
        ),
        ("velnor.role".to_owned(), "home".to_owned()),
    ]);
    let valid = VolumeIdentityCase {
        actual_name: name,
        labels: &labels,
        expected_name: name,
        connection,
        reservation,
        operation,
        role: "runner-home",
    };
    for case in [
        VolumeIdentityCase {
            actual_name: "foreign-home",
            ..valid
        },
        VolumeIdentityCase {
            connection: "foreign",
            ..valid
        },
        VolumeIdentityCase {
            reservation: reservation + 1,
            ..valid
        },
        VolumeIdentityCase {
            operation: operation + 1,
            ..valid
        },
        VolumeIdentityCase {
            role: "runner-work",
            ..valid
        },
        VolumeIdentityCase {
            labels: &no_labels,
            ..valid
        },
        VolumeIdentityCase {
            labels: &old_labels,
            ..valid
        },
    ] {
        assert!(!identity_matches(case));
    }
}

#[derive(Clone, Copy)]
struct VolumeIdentityCase<'a> {
    actual_name: &'a str,
    labels: &'a HashMap<String, String>,
    expected_name: &'a str,
    connection: &'a str,
    reservation: i64,
    operation: i64,
    role: &'a str,
}

fn created_home_labels(
    connection: &str,
    reservation: i64,
    operation: i64,
) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.connection".to_owned(), connection.to_owned()),
        ("velnor.reservation".to_owned(), reservation.to_string()),
        ("velnor.operation".to_owned(), operation.to_string()),
        ("velnor.role".to_owned(), "runner-home".to_owned()),
    ])
}

fn identity_matches(case: VolumeIdentityCase<'_>) -> bool {
    super::volume_identity_matches(
        case.actual_name,
        case.labels,
        case.expected_name,
        case.connection,
        case.reservation,
        case.operation,
        case.role,
    )
}
