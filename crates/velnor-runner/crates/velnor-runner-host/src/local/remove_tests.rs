// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::remove_labelled;
use crate::error::HostError;

fn unreachable_docker() -> Result<bollard::Docker, HostError> {
    crate::docker_client::connect_unix("/dev/null")
}

#[tokio::test]
async fn removal_refuses_a_filter_that_would_match_every_container() -> Result<(), HostError> {
    let docker = unreachable_docker()?;
    for labels in [vec![], vec![String::new()]] {
        assert_eq!(
            remove_labelled(&docker, &labels).await,
            Err(HostError::Ownership)
        );
    }
    assert_eq!(
        remove_labelled(&docker, &["velnor-local=x".to_owned()]).await,
        Err(HostError::Docker)
    );
    Ok(())
}
