//! Post-delete absence failures do not become cleanup proof.

use super::*;

#[tokio::test]
async fn done_post_delete_non_404_keeps_cleanup_unproven() -> Result<(), String> {
    let scratch = crate::launch_harness::Scratch::new("volume-post-delete")
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

    let stub = DockerStub::open(pair_cleanup_responses())?;
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
    assert_eq!(requests.len(), 18);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Done);
    assert!(rows[0].docker_id.is_some());
    assert!(rows[0].dind_id.is_some());
    assert!(!rows[0].cleanup_proven);
    Ok(())
}

fn pair_cleanup_responses() -> Vec<Response> {
    let mut responses = vec![
        http(
            200,
            &container_json("runner-id", Some(WORKER), Some("runner")),
        ),
        http(200, &container_json("dind-id", Some(WORKER), Some("dind"))),
        http(200, r#"{"State":{"Running":false}}"#),
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
    for (index, (name, role)) in volume_names().into_iter().enumerate() {
        responses.push(http(200, &volume_json(name, WORKER, role)));
        responses.push(http(204, ""));
        let status = if index == 2 { 500 } else { 404 };
        responses.push(http(status, r#"{"message":"not absent"}"#));
    }
    responses
}
