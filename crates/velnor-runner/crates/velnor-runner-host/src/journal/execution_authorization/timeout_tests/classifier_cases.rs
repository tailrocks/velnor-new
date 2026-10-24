use super::*;

#[tokio::test]
async fn timeout_classifier_requires_exact_live_started_pending_proof() -> Result<(), HostError> {
    let path = std::env::temp_dir().join(format!(
        "velnor-timeout-classifier-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_nanos()
    ));
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    create_schema(&connection).await?;
    let channel = test_channel()?;
    let channel_json =
        String::from_utf8(channel.canonical_json()?).map_err(|_| HostError::Journal)?;
    let channel_digest = channel.computed_digest()?;
    insert_live_rows(&connection, &channel, &channel_json, &channel_digest).await?;
    assert_timeout_classifier_states(&connection, &channel, &channel_json, &channel_digest).await?;
    drop(connection);
    drop(database);
    std::fs::remove_file(path).map_err(|_| HostError::Journal)?;
    Ok(())
}
