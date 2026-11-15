// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::models::{ContainerConfig, ContainerInspectResponse, ContainerState};

use super::validate_owned_container;
use crate::error::HostError;

const ID: &str = "56cf5a2b40274af8ede1b5920167014cdeb3e2b51b5d048b3a4f6c43737c4931";
const NAME: &str = "wfccd1ef74cea1c944e3a8e3145ffdb8b-runner";

fn pair_labels() -> HashMap<String, String> {
    HashMap::from([
        ("io.velnor.managed".to_owned(), "true".to_owned()),
        ("io.velnor.reservation-id".to_owned(), "1".to_owned()),
        ("io.velnor.operation-id".to_owned(), "2".to_owned()),
        ("io.velnor.connection-id".to_owned(), "lab".to_owned()),
        ("io.velnor.role".to_owned(), "runner".to_owned()),
    ])
}

fn image_labels() -> HashMap<String, String> {
    HashMap::from([
        ("io.velnor.image.kind".to_owned(), "runner".to_owned()),
        (
            "org.opencontainers.image.revision".to_owned(),
            "9a2c7a382932fd62802b19acd0048e5b4b5528e6".to_owned(),
        ),
        (
            "org.opencontainers.image.title".to_owned(),
            "ubuntu".to_owned(),
        ),
    ])
}

fn inspected(labels: HashMap<String, String>, running: bool) -> ContainerInspectResponse {
    ContainerInspectResponse {
        id: Some(ID.to_owned()),
        name: Some(format!("/{NAME}")),
        config: Some(ContainerConfig {
            labels: Some(labels),
            ..ContainerConfig::default()
        }),
        state: Some(ContainerState {
            running: Some(running),
            ..ContainerState::default()
        }),
        ..ContainerInspectResponse::default()
    }
}

fn check(inspected: &ContainerInspectResponse) -> Result<(), HostError> {
    validate_owned_container(inspected, ID, NAME, "runner", (1, 2), "lab")
}

#[test]
fn a_running_pair_container_that_carries_its_image_labels_is_owned() {
    let mut labels = pair_labels();
    labels.extend(image_labels());
    assert_eq!(check(&inspected(labels, true)), Ok(()));
}

#[test]
fn a_container_with_another_reservation_is_refused() {
    let mut labels = pair_labels();
    labels.extend(image_labels());
    labels.insert("io.velnor.reservation-id".to_owned(), "9".to_owned());
    assert_eq!(check(&inspected(labels, true)), Err(HostError::Ownership));
}

#[test]
fn a_stopped_pair_container_is_refused() {
    assert_eq!(
        check(&inspected(pair_labels(), false)),
        Err(HostError::Ownership)
    );
}
