// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    path::Path,
    time::{Duration, Instant},
};

use super::support::{self, Fixture};

#[test]
fn public_job_publication_releases_verified_readback_before_next_blob_finishes()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new_with_large_raw_tar()?;
    let layer_digest = fixture
        .layer_digests
        .first()
        .ok_or("large image fixture has no layer")?;
    let (ready, release) = support::readback_barrier::configure(&fixture, layer_digest)?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;
    std::thread::scope(|scope| {
        let publication = scope.spawn(|| fixture.invoke());
        let live = wait_for_file(&ready).and_then(|()| live_scratch_state(&fixture, layer_digest));
        let release_result = fs::write(&release, b"continue");
        let output = publication
            .join()
            .map_err(|_| std::io::Error::other("publication thread panicked"))??;
        let (config_absent, layer_readback_present, layout_present) = live?;
        release_result?;
        assert!(
            output.status.success(),
            "publication failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(fixture.last_fixture_error()?.is_none());
        assert!(fixture.receipt.is_file());
        let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.receipt)?)?;
        assert_eq!(receipt["schema"], "velnor.image-publication.v1");
        assert_eq!(receipt["registry_digest"], fixture.expected_digest);
        assert!(
            config_absent,
            "verified config readback remained in scratch"
        );
        assert!(
            layer_readback_present,
            "blocked layer readback was not present"
        );
        assert!(layout_present, "original OCI layout was not present");
        assert_eq!(
            fixture.registry_digest()?,
            Some(fixture.expected_digest.clone())
        );
        assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
        assert_eq!(fs::read(&fixture.archive)?, archive_before);
        assert_eq!(fs::read(&fixture.record)?, record_before);
        assert_eq!(fs::read(&foreign)?, b"preserve");
        assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
        Ok(())
    })
}

fn live_scratch_state(
    fixture: &Fixture,
    layer_digest: &str,
) -> Result<(bool, bool, bool), Box<dyn Error>> {
    let mut directories = fs::read_dir(&fixture.scratch)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| path.is_dir());
    let publication = directories
        .next()
        .ok_or("publication directory was not present at the barrier")?;
    if directories.next().is_some() {
        return Err("unexpected extra publication directory".into());
    }
    let layout = publication.join("oci");
    let config_path = layout_blob_path(&layout, &fixture.config_digest)?;
    let layer_path = layout_blob_path(&layout, layer_digest)?;
    Ok((
        !publication.join("crane-readback-00000000.blob").exists(),
        publication.join("crane-readback-00000001.blob").is_file(),
        layout.join("oci-layout").is_file() && config_path.is_file() && layer_path.is_file(),
    ))
}

fn layout_blob_path(layout: &Path, digest: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or("fixture digest has no sha256 prefix")?;
    Ok(layout.join("blobs").join("sha256").join(hex))
}

fn wait_for_file(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.is_file() {
        if Instant::now() >= deadline {
            return Err("Crane fixture did not reach the blob readback barrier".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
