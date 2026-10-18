use std::collections::HashMap;
use std::sync::Arc;

use bollard::Docker;
use bollard::models::{ContainerConfig, SystemInfo};

use crate::journal::{ImageImportState, Journal};
use crate::launch_harness::Scratch;

use super::release::PreparedRelease;
use super::selected_daemon;
use super::{ProviderFailure, ReleaseImageProvider, build_pin_status};
use crate::launch::inspect_tests::{DockerStub, http};

fn prepared_release(source_revision: &str) -> PreparedRelease {
    let archive_sha256 = "f".repeat(64);
    PreparedRelease {
        load_archive: Vec::new(),
        source_revision: source_revision.to_owned(),
        archive_sha256: archive_sha256.clone(),
        archive_index_digest: "b".repeat(64),
        manifest_digest: format!("sha256:{}", "c".repeat(64)),
        manifest_size: 480,
        config_digest: format!("sha256:{}", "a".repeat(64)),
        config: ContainerConfig {
            user: Some("65532:65532".to_owned()),
            env: Some(vec![
                "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_owned(),
            ]),
            working_dir: Some("/".to_owned()),
            entrypoint: Some(vec!["/velnor/resource-probe".to_owned()]),
            labels: Some(HashMap::from([(
                "org.opencontainers.image.revision".to_owned(),
                source_revision.to_owned(),
            )])),
            ..Default::default()
        },
        load_archive_sha256: "1".repeat(64),
        load_archive_index_digest: "2".repeat(64),
        image_reference: format!("velnor-resource-probe:v{source_revision}-{archive_sha256}"),
    }
}

#[test]
fn provider_requires_a_valid_compiled_source_pin() {
    assert!(matches!(
        ReleaseImageProvider::with_source_revision("pat", "not-a-sha"),
        Err(ProviderFailure::TrustedSourcePinInvalid)
    ));
    assert!(matches!(
        ReleaseImageProvider::with_source_revision("", &"a".repeat(40)),
        Err(ProviderFailure::CredentialMissing)
    ));
    if let Err(reason) = build_pin_status() {
        assert!(matches!(
            reason,
            "trusted_source_pin_missing" | "trusted_source_pin_invalid"
        ));
    }
}

#[tokio::test]
async fn verifier_failure_stops_the_production_provider_path_before_docker() -> Result<(), String> {
    let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
        .map_err(|error| error.code().to_owned())?;
    let (_scratch, journal) = test_journal("provider-verifier-failure").await?;
    let docker = Docker::connect_with_defaults().map_err(|error| error.to_string())?;
    let info = SystemInfo {
        id: Some("selected-engine".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some("x86_64".to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..Default::default()
    };
    let result = provider
        .verified_candidate_with_release(
            &docker,
            &journal,
            "selected-engine",
            &info,
            std::future::ready(Err(ProviderFailure::Attestation)),
        )
        .await;
    assert!(matches!(result, Err(ProviderFailure::Attestation)));
    Ok(())
}

#[tokio::test]
async fn release_delay_and_engine_change_block_image_inspection_and_import() -> Result<(), String> {
    let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
        .map_err(|error| error.code().to_owned())?;
    let (_scratch, journal) = test_journal("provider-engine-change").await?;
    let selected = SystemInfo {
        id: Some("selected-engine".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some("x86_64".to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..Default::default()
    };
    let replacement = r#"{"ID":"replacement-engine","OSType":"linux","Architecture":"x86_64","DockerRootDir":"/var/lib/docker"}"#;
    let docker = DockerStub::open(vec![http(200, replacement)])?;
    let release = async {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        Ok(Arc::new(prepared_release(&"a".repeat(40))))
    };

    let result = provider
        .verified_candidate_with_release(
            &docker.docker,
            &journal,
            "selected-engine",
            &selected,
            release,
        )
        .await;
    let requests = docker.finish().await?;

    assert!(matches!(result, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET ") && requests[0].contains("/info"));
    Ok(())
}

#[tokio::test]
async fn architecture_change_during_release_preparation_blocks_image_load() -> Result<(), String> {
    let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
        .map_err(|error| error.code().to_owned())?;
    let (_scratch, journal) = test_journal("provider-architecture-change").await?;
    let selected = SystemInfo {
        id: Some("selected-engine".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some("aarch64".to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..Default::default()
    };
    let replacement = r#"{"ID":"selected-engine","OSType":"linux","Architecture":"x86_64","DockerRootDir":"/var/lib/docker"}"#;
    let docker = DockerStub::open(vec![http(200, replacement)])?;
    let release = async {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        Ok(Arc::new(prepared_release(&"a".repeat(40))))
    };

    let result = provider
        .verified_candidate_with_release(
            &docker.docker,
            &journal,
            "selected-engine",
            &selected,
            release,
        )
        .await;
    let requests = docker.finish().await?;

    assert!(matches!(result, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET ") && requests[0].contains("/info"));
    Ok(())
}

#[tokio::test]
async fn unresolved_import_rows_do_not_inspect_or_repeat_the_load() -> Result<(), String> {
    for (label, uncertain) in [("requested", false), ("uncertain", true)] {
        let (_scratch, journal) = test_journal(&format!("provider-import-{label}")).await?;
        journal
            .bind_engine("selected-engine")
            .await
            .map_err(|error| error.to_string())?;
        let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
            .map_err(|error| error.code().to_owned())?;
        let release = prepared_release(&"a".repeat(40));
        let info = SystemInfo {
            id: Some("selected-engine".to_owned()),
            os_type: Some("linux".to_owned()),
            architecture: Some("aarch64".to_owned()),
            docker_root_dir: Some("/var/lib/docker".to_owned()),
            ..Default::default()
        };
        let selected =
            selected_daemon(&info, "selected-engine").map_err(|error| error.code().to_owned())?;
        let request = super::image_import_request(&selected, &release)
            .map_err(|error| error.code().to_owned())?;
        let record = journal
            .prepare_image_import(request.clone())
            .await
            .map_err(|error| error.to_string())?;
        let claim = journal
            .claim_image_import_effect(record.id, &request)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "expected prepared image import claim".to_owned())?;
        if uncertain {
            journal
                .mark_image_import_uncertain(claim)
                .await
                .map_err(|error| error.to_string())?;
        }

        let docker = DockerStub::open(Vec::new())?;
        let result = provider
            .inspect_or_load(&docker.docker, &journal, &selected, &release)
            .await;
        let requests = docker.finish_observed().await?;
        assert!(matches!(result, Err(ProviderFailure::ImageLoad)));
        assert_eq!(requests, Vec::<String>::new());
        let replay = journal
            .prepare_image_import(request)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(
            replay.state,
            if uncertain {
                ImageImportState::Uncertain
            } else {
                ImageImportState::Requested
            }
        );
    }
    Ok(())
}

#[tokio::test]
async fn derived_reference_collision_stops_before_image_load() -> Result<(), String> {
    let (_scratch, journal) = test_journal("provider-derived-reference-collision").await?;
    journal
        .bind_engine("selected-engine")
        .await
        .map_err(|error| error.to_string())?;
    let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
        .map_err(|error| error.code().to_owned())?;
    let release = prepared_release(&"a".repeat(40));
    let info = SystemInfo {
        id: Some("selected-engine".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some("aarch64".to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..Default::default()
    };
    let selected =
        selected_daemon(&info, "selected-engine").map_err(|error| error.code().to_owned())?;
    let foreign_image = format!(
        r#"{{"Id":"sha256:{}","RepoTags":["{}"],"Os":"linux","Architecture":"amd64"}}"#,
        "9".repeat(64),
        release.image_reference
    );
    let docker = DockerStub::open(vec![http(200, &foreign_image)])?;

    let result = provider
        .inspect_or_load(&docker.docker, &journal, &selected, &release)
        .await;
    let requests = docker.finish().await?;
    assert!(matches!(result, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET ") && requests[0].contains("/images/"));
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("/images/load"))
    );

    let request = super::image_import_request(&selected, &release)
        .map_err(|error| error.code().to_owned())?;
    let row = journal
        .prepare_image_import(request)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(row.state, ImageImportState::Prepared);
    Ok(())
}

#[tokio::test]
async fn derived_reference_recheck_blocks_a_collision_after_claim() -> Result<(), String> {
    let (_scratch, journal) = test_journal("provider-derived-reference-race").await?;
    journal
        .bind_engine("selected-engine")
        .await
        .map_err(|error| error.to_string())?;
    let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
        .map_err(|error| error.code().to_owned())?;
    let release = prepared_release(&"a".repeat(40));
    let selected_info = r#"{"ID":"selected-engine","OSType":"linux","Architecture":"aarch64","DockerRootDir":"/var/lib/docker"}"#;
    let colliding_image = format!(
        r#"{{"Id":"sha256:{}","Os":"linux","Architecture":"amd64"}}"#,
        "9".repeat(64)
    );
    let docker = DockerStub::open(vec![
        http(404, r#"{"message":"not found"}"#),
        http(404, r#"{"message":"not found"}"#),
        http(404, r#"{"message":"not found"}"#),
        http(200, selected_info),
        http(200, selected_info),
        http(200, &colliding_image),
    ])?;
    let info = SystemInfo {
        id: Some("selected-engine".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some("aarch64".to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..Default::default()
    };
    let selected =
        selected_daemon(&info, "selected-engine").map_err(|error| error.code().to_owned())?;

    let result = provider
        .inspect_or_load(&docker.docker, &journal, &selected, &release)
        .await;
    let requests = docker.finish().await?;
    assert!(matches!(result, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 6);
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("/images/load"))
    );
    assert!(requests[5].starts_with("GET ") && requests[5].contains("/images/"));

    let request = super::image_import_request(&selected, &release)
        .map_err(|error| error.code().to_owned())?;
    let row = journal
        .prepare_image_import(request)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(row.state, ImageImportState::Uncertain);
    Ok(())
}

mod image_identity;

async fn test_journal(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    Ok((scratch, journal))
}
