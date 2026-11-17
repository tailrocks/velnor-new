// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::{TestResult, wait_for_decision_bytes};
use velnor_resource_probe::job_hook::Decision;

const DOCUMENT: &[u8] = br#"{"schema_version":1,"allow":false,"challenge":"a","request_sha256":"b","reservation_id":11,"intent_id":22,"actual_request_id":33,"run_id":44,"run_attempt":1,"job_key":"ci","runner_id":55,"policy_generation":1,"policy_digest":"c","native_job_id":null,"rest_job_id":null,"deny_code":"request_mismatch"}"#;

#[tokio::test]
async fn decision_reader_waits_for_a_complete_document_after_a_partial_read() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("decision.json");
    let expected: Decision = serde_json::from_slice(DOCUMENT)?;
    let split = DOCUMENT.len() / 2;
    let prefix = DOCUMENT[..split].to_vec();
    let suffix = DOCUMENT[split..].to_vec();
    std::fs::write(&path, &prefix)?;
    let (observed_tx, mut observed_rx) = tokio::sync::mpsc::unbounded_channel();
    let writer_path = path.clone();
    let writer = tokio::spawn(async move {
        observed_rx
            .recv()
            .await
            .ok_or_else(|| std::io::Error::other("reader did not read the prefix"))?;
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(writer_path)
            .await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &suffix).await?;
        Ok::<(), std::io::Error>(())
    });
    let read_path = path.clone();
    let read_prefix = prefix.clone();
    let download = move || {
        let path = read_path.clone();
        let prefix = read_prefix.clone();
        let observed = observed_tx.clone();
        async move {
            let bytes = tokio::fs::read(path).await?;
            if bytes == prefix {
                let _sent = observed.send(());
            }
            Ok(bytes)
        }
    };
    let observed = wait_for_decision_bytes(download, Duration::from_secs(1)).await;
    writer.await??;
    assert_eq!(observed?, expected);
    Ok(())
}

#[tokio::test]
async fn decision_reader_times_out_on_a_document_that_stays_incomplete() {
    let partial = b"{\"schema_version\":1".to_vec();
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        wait_for_decision_bytes(
            move || {
                let partial = partial.clone();
                async move { Ok(partial) }
            },
            Duration::from_millis(20),
        ),
    )
    .await
    .expect("reader must honor its deadline");
    let error = result.expect_err("incomplete JSON must time out");
    assert!(
        error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::TimedOut)
    );
}
