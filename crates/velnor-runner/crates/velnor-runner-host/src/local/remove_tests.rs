// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{remove_audited, remove_labelled};
use crate::error::HostError;

fn unreachable_docker() -> Result<bollard::Docker, HostError> {
    crate::docker_client::connect_unix("/dev/null")
}

#[tokio::test]
async fn removal_refuses_a_filter_that_would_match_every_container() -> Result<(), HostError> {
    let docker = unreachable_docker()?;
    let directory = tempfile::tempdir().map_err(|_| HostError::Cleanup)?;
    let path = directory.path().join("cleanup.jsonl");
    for labels in [
        vec![],
        vec![String::new()],
        vec!["velnor-local=x".to_owned(), String::new()],
    ] {
        assert_eq!(
            remove_labelled(&docker, &labels).await,
            Err(HostError::Ownership)
        );
        assert_eq!(
            remove_audited(&docker, &labels, &path).await,
            Err(HostError::Ownership)
        );
        assert!(!path.exists());
    }
    assert_eq!(
        remove_labelled(&docker, &["velnor-local=x".to_owned()]).await,
        Err(HostError::Docker)
    );
    assert_eq!(
        remove_audited(&docker, &["velnor-local=x".to_owned()], &path).await,
        Err(HostError::Docker)
    );
    let record = std::fs::read_to_string(path).map_err(|_| HostError::Cleanup)?;
    let record: serde_json::Value =
        serde_json::from_str(&record).map_err(|_| HostError::Cleanup)?;
    assert_eq!(
        record,
        serde_json::json!({"event": "intent", "filters": {"label": ["velnor-local=x"]}})
    );
    Ok(())
}
