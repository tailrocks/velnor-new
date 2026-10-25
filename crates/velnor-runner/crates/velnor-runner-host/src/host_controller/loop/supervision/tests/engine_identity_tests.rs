use super::{
    LocalWorkerHealthSupervisor, RunningInspectServer, all_candidates, selected_engine,
    supervise_local_worker_health,
};
use crate::{
    error::HostError, journal::request_lifecycle::tests::helpers,
    launch::bootstrap::cleanup_runtime_fixture,
};

#[test]
fn replaced_docker_identity_keeps_a_missing_runner_held() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "worker-health-replaced-docker-identity",
        || {
            Box::pin(async {
                Box::pin(replaced_docker_identity_keeps_a_missing_runner_held_inner())
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn replaced_docker_identity_keeps_a_missing_runner_held_inner() -> Result<(), HostError> {
    let fixture = cleanup_runtime_fixture().await?;
    fixture.base.record_started().await?;
    let binding = helpers::binding("native-started-intake", 101)?;
    let candidates = all_candidates(&fixture.scope).await?;
    assert_eq!(candidates.len(), 1);
    let docker = fixture.server.docker();
    let selected = selected_engine(&fixture.scope, &docker).await?;
    let info = docker.info().await.map_err(|_| HostError::Docker)?;
    for replacement in [
        Replacement::Engine,
        Replacement::Root,
        Replacement::InfoError,
    ] {
        let mut replaced_info = info.clone();
        match replacement {
            Replacement::Engine => replaced_info.id = Some("replacement-engine".to_owned()),
            Replacement::Root => {
                replaced_info.docker_root_dir = Some("/var/lib/docker/replacement".to_owned());
            }
            Replacement::InfoError => {}
        }
        let (missing_docker, _server) = match replacement {
            Replacement::InfoError => {
                RunningInspectServer::open_missing_with_info_error(&replaced_info)?
            }
            Replacement::Engine | Replacement::Root => {
                RunningInspectServer::open_missing(&replaced_info)?
            }
        };
        let mut supervisor = LocalWorkerHealthSupervisor::default();
        supervise_local_worker_health(
            &mut supervisor,
            &fixture.journal,
            &missing_docker,
            &selected,
            std::slice::from_ref(&binding),
            &[],
        )
        .await;
        assert_eq!(
            fixture.scope.unresolved_local_worker_exits().await?,
            Vec::new()
        );
        assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Replacement {
    Engine,
    Root,
    InfoError,
}
