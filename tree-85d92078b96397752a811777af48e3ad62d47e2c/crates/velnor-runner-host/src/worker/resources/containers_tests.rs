//! Docker inspect label tests, including image-inherited OCI labels.

use std::collections::HashMap;

use bollard::models::{ContainerConfig, ContainerInspectResponse};

use crate::error::HostError;
use crate::journal::LaunchIdentity;
use crate::worker::{dind_create, label_map, runner_create_for_identity};

use super::{inspect_labels_match, same_launch, validate_existing_row};

fn identity() -> Result<LaunchIdentity, HostError> {
    LaunchIdentity::new(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        7,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "engine-test",
    )
}

fn inspect_with_labels(labels: HashMap<String, String>) -> ContainerInspectResponse {
    ContainerInspectResponse {
        config: Some(ContainerConfig {
            labels: Some(labels),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn inspect_accepts_inherited_oci_labels_with_exact_velnor_identity() -> Result<(), HostError> {
    let spec = dind_create(&identity()?)?;
    let expected = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    let mut actual = expected.clone();
    actual.insert(
        "org.opencontainers.image.version".to_owned(),
        "26.04".to_owned(),
    );
    let inspected = inspect_with_labels(actual.clone());

    assert!(inspect_labels_match(&expected, &inspected)?);
    assert_eq!(validate_existing_row(&expected, &actual)?, "dind");
    Ok(())
}

#[test]
fn runner_preflight_accepts_its_existing_dind_role() -> Result<(), HostError> {
    let identity = identity()?;
    let runner = runner_create_for_identity(&identity, None)?;
    let dind = dind_create(&identity)?;
    let expected = label_map(&runner.labels)?.ok_or(HostError::Ownership)?;
    let mut actual = label_map(&dind.labels)?.ok_or(HostError::Ownership)?;
    actual.insert(
        "org.opencontainers.image.version".to_owned(),
        "26.04".to_owned(),
    );

    assert_eq!(
        expected.get("velnor.role").map(String::as_str),
        Some("runner")
    );
    assert_eq!(actual.get("velnor.role").map(String::as_str), Some("dind"));
    assert!(same_launch(&expected, &actual));
    assert_eq!(validate_existing_row(&expected, &actual)?, "dind");
    Ok(())
}

#[test]
fn inspect_rejects_mismatched_velnor_identity_values() -> Result<(), HostError> {
    let spec = dind_create(&identity()?)?;
    let expected = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    let mut actual = expected.clone();
    actual.insert(
        "velnor.launch".to_owned(),
        "cccccccccccccccccccccccccccccccc".to_owned(),
    );

    assert!(!inspect_labels_match(
        &expected,
        &inspect_with_labels(actual.clone())
    )?);
    assert!(!same_launch(&expected, &actual));
    Ok(())
}

#[test]
fn inspect_rejects_foreign_velnor_labels() -> Result<(), HostError> {
    let spec = dind_create(&identity()?)?;
    let expected = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    let mut actual = expected.clone();
    actual.insert("velnor.owner".to_owned(), "foreign".to_owned());

    assert!(!inspect_labels_match(
        &expected,
        &inspect_with_labels(actual.clone())
    )?);
    assert!(!same_launch(&expected, &actual));
    Ok(())
}
