use bollard::models::SystemInfo;

use crate::error::HostError;
use crate::journal::EngineContext;
use crate::launch::SelectedEngineContext;
use crate::launch::resource_probe::projection::DockerRoot;
use crate::launch::verified_images::VerifiedWorkerImages;

use super::{fixture_proofs, valid_release_secret};
use crate::launch::resource_probe::provider::identity::SelectedDaemon;

const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DOCKER_ROOT: &str = "/var/lib/docker";

#[test]
fn verified_runner_and_dind_proofs_bind_to_one_engine_and_source() -> Result<(), HostError> {
    let selected = selected_daemon("selected-engine", "aarch64");
    let expected = selected_context(&selected);
    let (runner, dind) = fixture_proofs(&selected, SOURCE_SHA);

    let images = VerifiedWorkerImages::from_verified_proofs(&expected, SOURCE_SHA, runner, dind)?;

    assert_eq!(images.engine_id(), "selected-engine");
    assert_eq!(images.daemon_architecture(), "aarch64");
    assert_eq!(images.platform(), "linux/amd64");
    assert_eq!(images.source_sha(), SOURCE_SHA);
    assert_eq!(
        images.runner().runtime_id(),
        format!("sha256:{}", "1".repeat(64))
    );
    assert_eq!(
        images.dind().runtime_id(),
        format!("sha256:{}", "5".repeat(64))
    );
    Ok(())
}

#[test]
fn verified_proofs_reject_wrong_engine_source_or_image_role() -> Result<(), HostError> {
    let selected = selected_daemon("selected-engine", "aarch64");
    let expected = selected_context(&selected);
    let (runner, dind) = fixture_proofs(&selected, SOURCE_SHA);
    assert!(matches!(
        VerifiedWorkerImages::from_verified_proofs(
            &expected,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            runner,
            dind
        ),
        Err(HostError::Identity)
    ));

    let (runner, dind) = fixture_proofs(&selected, SOURCE_SHA);
    assert!(matches!(
        VerifiedWorkerImages::from_verified_proofs(&expected, SOURCE_SHA, dind, runner),
        Err(HostError::Identity)
    ));

    let other = selected_daemon("different-engine", "aarch64");
    let (runner, dind) = fixture_proofs(&other, SOURCE_SHA);
    assert!(matches!(
        VerifiedWorkerImages::from_verified_proofs(&expected, SOURCE_SHA, runner, dind),
        Err(HostError::Identity)
    ));
    Ok(())
}

#[test]
fn release_secret_must_be_a_nonempty_single_token() {
    assert!(valid_release_secret("ghs_private_token"));
    assert!(!valid_release_secret(""));
    assert!(!valid_release_secret("  "));
    assert!(!valid_release_secret("ghs_token\n"));
}

fn selected_daemon(engine_id: &str, architecture: &str) -> SelectedDaemon {
    SelectedDaemon {
        engine_id: engine_id.to_owned(),
        docker_root_digest: DockerRoot::parse(DOCKER_ROOT)
            .expect("Docker root parses")
            .digest()
            .to_owned(),
        architecture: architecture.to_owned(),
    }
}

fn selected_context(selected: &SelectedDaemon) -> SelectedEngineContext {
    let engine = EngineContext {
        engine_id: selected.engine_id.clone(),
        endpoint: "unix:///var/run/docker.sock".to_owned(),
        platform: "linux/amd64".to_owned(),
    };
    let info = SystemInfo {
        id: Some(selected.engine_id.clone()),
        architecture: Some(selected.architecture.clone()),
        docker_root_dir: Some(DOCKER_ROOT.to_owned()),
        os_type: Some("linux".to_owned()),
        ..SystemInfo::default()
    };
    SelectedEngineContext::capture(&engine, &info).expect("selected daemon context captures")
}
