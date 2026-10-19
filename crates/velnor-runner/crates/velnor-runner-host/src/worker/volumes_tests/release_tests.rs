//! Post-delete absence failures do not become cleanup proof.

use super::*;

#[tokio::test]
async fn done_post_delete_non_404_keeps_cleanup_unproven() -> Result<(), String> {
    let volume_count = volume_names().len();
    let volume_failure_indexes = [3, volume_count - 1];

    for failed_volume_index in volume_failure_indexes {
        let scratch = crate::launch_harness::Scratch::new(&format!(
            "volume-post-delete-{failed_volume_index}"
        ))
        .map_err(|error| error.to_string())?;
        let journal = crate::Journal::open(&scratch.file())
            .await
            .map_err(|error| error.to_string())?;
        let (row, fresh) = journal
            .begin_launch("offer-transport")
            .await
            .map_err(|error| error.to_string())?;
        assert!(fresh);
        journal
            .bind_worker_volume(row, WORKER)
            .await
            .map_err(|error| error.to_string())?;
        journal
            .finish(row, Outcome::Done)
            .await
            .map_err(|error| error.to_string())?;

        let stub = DockerStub::open(pair_cleanup_responses(failed_volume_index))?;
        let decision = admission(
            &stub.docker,
            &journal,
            1,
            1,
            0,
            &crate::launch_harness::assigned_wait(1, 1),
        )
        .await;
        let requests = stub.finish().await?;

        assert_eq!(
            decision,
            Err(crate::EnsureError::Unexpected {
                status: 0,
                step: "docker"
            })
        );
        let completed_volume_count = failed_volume_index + 1;
        let expected_requests = 9 + 3 * completed_volume_count;
        assert_eq!(requests.len(), expected_requests);
        let rows = journal.rows().await.map_err(|error| error.to_string())?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, IntentState::Done);
        assert!(rows[0].docker_id.is_some());
        assert!(rows[0].dind_id.is_some());
        assert!(!rows[0].cleanup_proven);
        assert!(
            !journal
                .is_drained()
                .await
                .map_err(|error| error.to_string())?
        );
        assert_eq!(
            requests
                .iter()
                .any(|request| request.contains("wtransport-metadata")),
            failed_volume_index == volume_count - 1
        );
    }
    Ok(())
}

fn pair_cleanup_responses(failed_volume_index: usize) -> Vec<Response> {
    let mut responses = vec![
        http(
            200,
            &container_json("runner-id", Some(WORKER), Some("runner")),
        ),
        http(200, &container_json("dind-id", Some(WORKER), Some("dind"))),
        http(200, r#"{"State":{"Status":"exited","Running":false}}"#),
        http(
            200,
            &container_json("runner-id", Some(WORKER), Some("runner")),
        ),
        http(204, ""),
        http(404, r#"{"message":"missing"}"#),
        http(200, &container_json("dind-id", Some(WORKER), Some("dind"))),
        http(204, ""),
        http(404, r#"{"message":"missing"}"#),
    ];
    for (index, (name, role)) in volume_names()
        .into_iter()
        .enumerate()
        .take(failed_volume_index + 1)
    {
        responses.push(http(200, &volume_json(name, WORKER, role)));
        responses.push(http(204, ""));
        let status = if index == failed_volume_index {
            500
        } else {
            404
        };
        responses.push(http(status, r#"{"message":"not absent"}"#));
    }
    responses
}
