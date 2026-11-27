// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Result, anyhow};

use super::proto::Response;
use super::tests::{Fixture, TAR, begin, failed, version, write_all};

fn private(path: &std::path::Path) -> Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn streamed_dir(fixture: &Fixture) -> Result<PathBuf> {
    let [name] = fixture
        .incoming_names()
        .try_into()
        .map_err(|names| anyhow!("expected one upload directory, found {names:?}"))?;
    Ok(fixture.incoming().join(name))
}

#[tokio::test]
async fn an_upload_directory_swapped_for_another_one_is_never_published() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut stream = begin(&fixture.broker, "k-1").await?;
    let dir = streamed_dir(&fixture)?;
    let original = fixture.scratch.0.join("original");
    fs::rename(&dir, &original)?;
    fs::create_dir(&dir)?;
    fs::hard_link(original.join("data.tar"), dir.join("data.tar"))?;
    let count = write_all(&mut stream, TAR.to_vec()).await?;
    let response = stream.seal(count).await;
    failed(&response, "not the directory this broker streamed into");
    fixture.assert_nothing_published();
    fixture.assert_no_leftover();
    Ok(())
}

#[tokio::test]
async fn a_data_file_replaced_or_purged_while_publishing_is_never_published() -> Result<()> {
    for purged in [false, true] {
        let fixture = Fixture::new()?;
        let mut stream = begin(&fixture.broker, "k-1").await?;
        let dir = streamed_dir(&fixture)?;
        fs::remove_file(dir.join("data.tar"))?;
        if !purged {
            fs::write(dir.join("data.tar"), b"somebody else's bytes")?;
        }
        let count = write_all(&mut stream, TAR.to_vec()).await?;
        let response = stream.seal(count).await;
        let message = if purged {
            "inspect the published cache data"
        } else {
            "not the file this broker streamed"
        };
        failed(&response, message);
        fixture.assert_nothing_published();
        fixture.assert_no_leftover();
    }
    Ok(())
}

#[tokio::test]
async fn a_shared_incoming_directory_is_refused_before_a_pipe_is_handed_out() -> Result<()> {
    let fixture = Fixture::new()?;
    let incoming = fixture.root().join("incoming");
    private(&incoming)?;
    fs::set_permissions(&incoming, fs::Permissions::from_mode(0o755))?;
    let refused = fixture.broker.begin_cache("k-1", &version()).await;
    assert!(
        matches!(&refused, Err(Response::Failed { message }) if message.contains("the incoming directory is not owned")),
        "{:?}",
        refused.err()
    );
    Ok(())
}

#[tokio::test]
async fn a_shared_incoming_directory_of_this_broker_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    private(&fixture.root().join("incoming"))?;
    fs::create_dir(fixture.incoming())?;
    fs::set_permissions(fixture.incoming(), fs::Permissions::from_mode(0o755))?;
    let refused = fixture.broker.begin_cache("k-1", &version()).await;
    assert!(
        matches!(&refused, Err(Response::Failed { message }) if message.contains("of this broker is not owned")),
        "{:?}",
        refused.err()
    );
    Ok(())
}
