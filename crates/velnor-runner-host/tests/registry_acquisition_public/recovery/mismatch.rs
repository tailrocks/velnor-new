// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

async fn snapshot(path: &std::path::Path) -> TestResult<Vec<String>> {
    let connection = turso::Builder::new_local(path.to_str().ok_or("journal path")?)
        .build()
        .await?
        .connect()?;
    let mut rows = connection.query(
        "SELECT quote(id)||'|'||quote(engine_id)||'|'||quote(docker_root_digest)||'|'||quote(daemon_architecture)||'|'||quote(source_sha)||'|'||quote(release_manifest_sha256)||'|'||quote(role)||'|'||quote(registry_digest)||'|'||quote(config_digest)||'|'||quote(attempt)||'|'||quote(state)||'|'||quote(runtime_id)||'|'||quote(receipt) FROM image_import_registry ORDER BY id", ()
    ).await?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await? {
        records.push(row.get::<String>(0)?);
    }
    Ok(records)
}

#[tokio::test]
async fn mismatched_local_image_blocks_recovery_without_changing_history() -> TestResult {
    for completed in [false, true] {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("journal.db");
        let socket = temp.path().join("docker.sock");
        let (image, _, config, _) = fixture()?;
        if completed {
            let journal = Journal::open(&path).await?;
            journal.bind_engine(ENGINE_ID).await?;
            let gate = Arc::new((Mutex::new(true), Condvar::new()));
            let peer = Peer::bind(&socket, pull_plan(true, &image, &config, &gate)?)?;
            let result = acquire_registry_image(
                &journal,
                socket.to_str().ok_or("socket")?,
                specification()?,
            )
            .await?;
            assert_eq!(result.runtime_id(), image.config_digest);
            drop(peer);
            drop(journal);
            std::fs::remove_file(&socket)?;
        } else {
            leave_requested(&temp, &image, &config).await?;
        }
        let before = snapshot(&path).await?;
        assert_eq!(before.len(), 1);
        assert!(before[0].contains(if completed {
            "'completed'"
        } else {
            "'requested'"
        }));
        let gate = Arc::new((Mutex::new(true), Condvar::new()));
        let mut plan = pull_plan(true, &image, &config, &gate)?;
        plan.inspect.pop_front();
        let response = plan.inspect.pop_front().ok_or("matching inspection")?;
        let mut inspection: serde_json::Value = serde_json::from_slice(&response.body_bytes())?;
        inspection["RootFS"]["Layers"][0] = json!(format!("sha256:{}", "f".repeat(64)));
        plan.inspect
            .push_back(Response::json_value(200, &inspection));
        plan.pull.clear();
        let peer = Peer::bind(&socket, plan)?;
        let journal = Journal::open(&path).await?;
        let result =
            acquire_registry_image(&journal, socket.to_str().ok_or("socket")?, specification()?)
                .await;
        assert!(result.is_err(), "mismatched image was admitted");
        assert!(
            !peer
                .requests()?
                .iter()
                .any(|request| request.method == "POST")
        );
        assert_eq!(snapshot(&path).await?, before, "journal history changed");
    }
    Ok(())
}
