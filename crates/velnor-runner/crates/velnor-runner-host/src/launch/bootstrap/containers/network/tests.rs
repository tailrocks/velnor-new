use std::collections::{BTreeMap, HashMap};

use bollard::models::{EndpointResource, NetworkInspect};

use super::{valid_network_id, valid_network_name, validate_network};

fn labels() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("io.velnor.managed".to_owned(), "true".to_owned()),
        (
            "io.velnor.connection-id".to_owned(),
            "connection-a".to_owned(),
        ),
        ("io.velnor.engine-id".to_owned(), "engine-a".to_owned()),
        ("io.velnor.operation-id".to_owned(), "8".to_owned()),
        ("io.velnor.reservation-id".to_owned(), "7".to_owned()),
    ])
}

fn network() -> NetworkInspect {
    NetworkInspect {
        name: Some("velnor-v0123456789abcdef0123456789abcdef-private".to_owned()),
        id: Some("a".repeat(64)),
        scope: Some("local".to_owned()),
        driver: Some("bridge".to_owned()),
        enable_ipv4: Some(true),
        enable_ipv6: Some(false),
        internal: Some(false),
        attachable: Some(false),
        ingress: Some(false),
        labels: Some(labels().into_iter().collect()),
        ..Default::default()
    }
}

#[test]
fn network_name_and_id_are_strict() {
    assert!(valid_network_name(
        "velnor-v0123456789abcdef0123456789abcdef-private"
    ));
    assert!(!valid_network_name("velnor-v../private"));
    assert!(valid_network_id(&"a".repeat(64)));
    assert!(!valid_network_id(&"A".repeat(64)));
}

#[test]
fn inspect_accepts_only_the_exact_empty_egress_network() {
    let valid = network();
    validate_network(
        &valid,
        valid.name.as_deref().expect("name"),
        valid.id.as_deref().expect("id"),
        &labels(),
    )
    .expect("owned private network");

    let mut internal = network();
    internal.internal = Some(true);
    assert!(
        validate_network(
            &internal,
            "velnor-v0123456789abcdef0123456789abcdef-private",
            &"a".repeat(64),
            &labels(),
        )
        .is_err()
    );

    let mut occupied = network();
    occupied.containers = Some(HashMap::from([(
        "foreign".to_owned(),
        EndpointResource::default(),
    )]));
    assert!(
        validate_network(
            &occupied,
            "velnor-v0123456789abcdef0123456789abcdef-private",
            &"a".repeat(64),
            &labels(),
        )
        .is_err()
    );

    let mut attachable = network();
    attachable.attachable = Some(true);
    assert!(
        validate_network(
            &attachable,
            "velnor-v0123456789abcdef0123456789abcdef-private",
            &"a".repeat(64),
            &labels(),
        )
        .is_err()
    );

    let mut ingress = network();
    ingress.ingress = Some(true);
    assert!(
        validate_network(
            &ingress,
            "velnor-v0123456789abcdef0123456789abcdef-private",
            &"a".repeat(64),
            &labels(),
        )
        .is_err()
    );

    let mut ipv6 = network();
    ipv6.enable_ipv6 = Some(true);
    assert!(
        validate_network(
            &ipv6,
            "velnor-v0123456789abcdef0123456789abcdef-private",
            &"a".repeat(64),
            &labels(),
        )
        .is_err()
    );

    let mut wrong_driver = network();
    wrong_driver.driver = Some("macvlan".to_owned());
    assert!(
        validate_network(
            &wrong_driver,
            "velnor-v0123456789abcdef0123456789abcdef-private",
            &"a".repeat(64),
            &labels(),
        )
        .is_err()
    );

    assert!(
        validate_network(
            &network(),
            "velnor-v0123456789abcdef0123456789abcdef-private",
            &"b".repeat(64),
            &labels(),
        )
        .is_err()
    );

    let mut foreign_labels = labels();
    foreign_labels.insert("io.velnor.connection-id".to_owned(), "other".to_owned());
    assert!(
        validate_network(
            &network(),
            "velnor-v0123456789abcdef0123456789abcdef-private",
            &"a".repeat(64),
            &foreign_labels,
        )
        .is_err()
    );
}
