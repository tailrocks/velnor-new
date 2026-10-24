use super::*;

#[tokio::test]
async fn partial_worker_resources_stay_uncertain_charged_and_unscoped() -> Result<(), String> {
    for partial in [
        PartialResource::Dind,
        PartialResource::WorkerVolume,
        PartialResource::Runner,
    ] {
        assert_partial_resource_is_quarantined(partial).await?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum PartialResource {
    Dind,
    WorkerVolume,
    Runner,
}

impl PartialResource {
    fn subject(self) -> &'static str {
        match self {
            Self::Dind => "partial-dind-only",
            Self::WorkerVolume => "partial-volume-only",
            Self::Runner => "partial-runner-only",
        }
    }

    fn connection_id(self) -> &'static str {
        match self {
            Self::Dind => "partial-dind-connection",
            Self::WorkerVolume => "partial-volume-connection",
            Self::Runner => "partial-runner-connection",
        }
    }

    async fn bind(self, journal: &Journal, id: i64) -> Result<(), String> {
        match self {
            Self::Dind => journal.bind_worker(id, None, Some("dind-container")).await,
            Self::WorkerVolume => {
                journal
                    .bind_worker_volume(id, "w00000000000000000000000000000000")
                    .await
            }
            Self::Runner => {
                journal
                    .bind_worker(id, Some("runner-container"), None)
                    .await
            }
        }
        .map_err(|error| error.to_string())
    }

    fn assert_row(self, row: &crate::reconcile::IntentRow, id: i64) {
        assert_eq!(row.id, id);
        assert_eq!(row.kind, "launch");
        assert_eq!(row.subject, self.subject());
        assert_eq!(row.state, IntentState::Uncertain);
        assert!(!row.cleanup_proven);
        assert_eq!(
            row.dind_id.as_deref(),
            matches!(self, Self::Dind).then_some("dind-container")
        );
        assert_eq!(
            row.docker_id.as_deref(),
            matches!(self, Self::Runner).then_some("runner-container")
        );
        assert_eq!(
            row.worker_volume.as_deref(),
            matches!(self, Self::WorkerVolume).then_some("w00000000000000000000000000000000")
        );
    }
}

async fn assert_partial_resource_is_quarantined(partial: PartialResource) -> Result<(), String> {
    let scratch = Scratch::new(partial.subject()).map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let (id, inserted) = journal
        .begin_launch(partial.subject())
        .await
        .map_err(|error| error.to_string())?;
    assert!(inserted);
    partial.bind(&journal, id).await?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;

    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    partial.assert_row(&rows[0], id);
    assert_eq!(
        journal
            .capacity_rows(1)
            .await
            .map_err(|error| error.to_string())?,
        rows
    );
    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding(partial.connection_id(), 101)
            .map_err(|error| error.to_string())?;
    assert!(matches!(
        journal.connection_scope(&binding).await,
        Err(crate::error::HostError::Journal)
    ));
    assert_eq!(
        super::connection_scope_count(&path, binding.connection_id.as_str()).await?,
        0
    );
    drop(journal);

    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let reopened_rows = reopened.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(reopened_rows, rows);
    partial.assert_row(&reopened_rows[0], id);
    assert_eq!(
        reopened
            .capacity_rows(1)
            .await
            .map_err(|error| error.to_string())?,
        reopened_rows
    );
    assert!(matches!(
        reopened.connection_scope(&binding).await,
        Err(crate::error::HostError::Journal)
    ));
    assert_eq!(
        super::connection_scope_count(&path, binding.connection_id.as_str()).await?,
        0
    );
    Ok(())
}
