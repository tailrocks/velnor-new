//! Bounded Bollard adapter for the guest sampler.

use std::io::{Cursor, Read};
use std::path::Path;

use bollard::Docker;
use bollard::models::SystemInfo;
use bollard::query_parameters::{
    DownloadFromContainerOptionsBuilder, RemoveContainerOptionsBuilder,
};
use futures_util::{Stream, StreamExt};

use crate::worker::resources::confirmed_not_found;

use super::super::{MAX_PROBE_OUTPUT_BYTES, PROBE_OUTPUT_PATH};
use super::{CreatedProbe, GuestDockerClient, GuestSampleFailure, ProbeContainer, ProbeImage};
use crate::worker::resources::guest::GuestProbeCreate;

const MAX_PROBE_ARCHIVE_BYTES: usize = 16 * 1024;

/// Bollard client restricted to the sampler's fixed probe operations.
#[derive(Debug, Clone)]
pub(super) struct BollardGuestDocker {
    docker: Docker,
}

impl BollardGuestDocker {
    /// Bind the adapter to the selected Docker client.
    pub(super) fn new(docker: Docker) -> Self {
        Self { docker }
    }
}

impl GuestDockerClient for BollardGuestDocker {
    async fn info(&self) -> Result<SystemInfo, GuestSampleFailure> {
        self.docker
            .info()
            .await
            .map_err(|_| GuestSampleFailure::Docker)
    }

    async fn inspect_image(
        &self,
        reference: &str,
    ) -> Result<Option<ProbeImage>, GuestSampleFailure> {
        let image = match self.docker.inspect_image(reference).await {
            Ok(image) => image,
            Err(error) if confirmed_not_found(&error) => return Ok(None),
            Err(_) => return Err(GuestSampleFailure::Docker),
        };
        let repo_tags = image.repo_tags.unwrap_or_default();
        Ok(Some(ProbeImage {
            id: image.id,
            repo_tags,
            os: image.os,
            architecture: image.architecture,
        }))
    }

    async fn inspect_container(
        &self,
        reference: &str,
    ) -> Result<Option<ProbeContainer>, GuestSampleFailure> {
        let inspected = match self.docker.inspect_container(reference, None).await {
            Ok(container) => container,
            Err(error) if confirmed_not_found(&error) => return Ok(None),
            Err(_) => return Err(GuestSampleFailure::Docker),
        };
        let labels = inspected
            .config
            .and_then(|config| config.labels)
            .unwrap_or_default();
        let state = inspected.state;
        Ok(Some(ProbeContainer {
            id: inspected.id,
            name: inspected.name,
            image: inspected.image,
            labels,
            running: state.as_ref().and_then(|state| state.running),
            exit_code: state.and_then(|state| state.exit_code),
        }))
    }

    async fn create_container(
        &self,
        request: GuestProbeCreate,
    ) -> Result<CreatedProbe, GuestSampleFailure> {
        let created = self
            .docker
            .create_container(Some(request.options), request.config)
            .await
            .map_err(|_| GuestSampleFailure::Docker)?;
        Ok(CreatedProbe {
            id: created.id,
            has_warnings: !created.warnings.is_empty(),
        })
    }

    async fn start_container(&self, id: &str) -> Result<(), GuestSampleFailure> {
        self.docker
            .start_container(id, None)
            .await
            .map_err(|_| GuestSampleFailure::Docker)
    }

    async fn read_output(
        &self,
        id: &str,
        limit: usize,
    ) -> Result<Option<Vec<u8>>, GuestSampleFailure> {
        let options = DownloadFromContainerOptionsBuilder::new()
            .path(PROBE_OUTPUT_PATH)
            .build();
        let archive = collect_bounded_body(
            self.docker.download_from_container(id, Some(options)),
            MAX_PROBE_ARCHIVE_BYTES,
            confirmed_not_found,
        )
        .await?;
        archive
            .map(|bytes| extract_probe_output(bytes, limit.min(MAX_PROBE_OUTPUT_BYTES)))
            .transpose()
    }

    async fn remove_container(&self, id: &str) -> Result<(), GuestSampleFailure> {
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        match self.docker.remove_container(id, Some(options)).await {
            Ok(()) => Ok(()),
            Err(error) if confirmed_not_found(&error) => Ok(()),
            Err(_) => Err(GuestSampleFailure::Docker),
        }
    }
}

async fn collect_bounded_body<S, C, E, F>(
    stream: S,
    limit: usize,
    is_missing: F,
) -> Result<Option<Vec<u8>>, GuestSampleFailure>
where
    S: Stream<Item = Result<C, E>>,
    C: AsRef<[u8]>,
    F: Fn(&E) -> bool,
{
    let mut bytes = Vec::with_capacity(limit);
    futures_util::pin_mut!(stream);
    while let Some(item) = stream.next().await {
        let chunk = match item {
            Ok(chunk) => chunk,
            Err(error) if bytes.is_empty() && is_missing(&error) => return Ok(None),
            Err(_) => return Err(GuestSampleFailure::Docker),
        };
        let chunk = chunk.as_ref();
        // Bollard yields raw archive body chunks here, without its log-frame decoder.
        bytes
            .len()
            .checked_add(chunk.len())
            .filter(|size| *size <= limit)
            .ok_or(GuestSampleFailure::OutputLimit)?;
        bytes.extend_from_slice(chunk);
    }
    Ok(Some(bytes))
}

fn extract_probe_output(
    archive_bytes: Vec<u8>,
    limit: usize,
) -> Result<Vec<u8>, GuestSampleFailure> {
    if archive_bytes.len() > MAX_PROBE_ARCHIVE_BYTES {
        return Err(GuestSampleFailure::OutputLimit);
    }
    let mut archive = tar::Archive::new(Cursor::new(archive_bytes));
    let mut entries = archive
        .entries()
        .map_err(|_| GuestSampleFailure::ProbeOutput)?;
    let mut entry = entries
        .next()
        .ok_or(GuestSampleFailure::ProbeOutput)?
        .map_err(|_| GuestSampleFailure::ProbeOutput)?;
    let entry_path = entry.path().map_err(|_| GuestSampleFailure::ProbeOutput)?;
    if !expected_archive_path(&entry_path) || !entry.header().entry_type().is_file() {
        return Err(GuestSampleFailure::ProbeOutput);
    }
    let size = entry.size();
    if size > u64::try_from(limit).map_err(|_| GuestSampleFailure::OutputLimit)? {
        return Err(GuestSampleFailure::OutputLimit);
    }
    let capacity = usize::try_from(size).map_err(|_| GuestSampleFailure::OutputLimit)?;
    let mut output = Vec::with_capacity(capacity);
    entry
        .read_to_end(&mut output)
        .map_err(|_| GuestSampleFailure::ProbeOutput)?;
    if output.len() != capacity {
        return Err(GuestSampleFailure::ProbeOutput);
    }
    drop(entry);
    if entries.next().is_some() {
        return Err(GuestSampleFailure::ProbeOutput);
    }
    Ok(output)
}

fn expected_archive_path(path: &Path) -> bool {
    path == Path::new("velnor-guest-resource-sample")
        || path == Path::new("tmp").join("velnor-guest-resource-sample")
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io::Cursor;

    use futures_util::stream;

    use super::{
        GuestSampleFailure, MAX_PROBE_ARCHIVE_BYTES, collect_bounded_body, extract_probe_output,
    };

    fn make_archive(path: &str, output: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        let size = u64::try_from(output.len())?;
        let mut header = tar::Header::new_gnu();
        header.set_path(path)?;
        header.set_size(size);
        header.set_mode(0o600);
        header.set_cksum();
        let mut archive = tar::Builder::new(Vec::new());
        archive.append(&header, Cursor::new(output))?;
        Ok(archive.into_inner()?)
    }

    #[tokio::test]
    async fn archive_limit_is_checked_before_a_chunk_is_appended() {
        let chunks = stream::iter([Ok::<_, ()>(vec![b'x'; MAX_PROBE_ARCHIVE_BYTES + 1])]);
        assert_eq!(
            collect_bounded_body(chunks, MAX_PROBE_ARCHIVE_BYTES, |_| false).await,
            Err(GuestSampleFailure::OutputLimit)
        );
    }

    #[test]
    fn archive_returns_only_the_fixed_probe_file() -> Result<(), Box<dyn Error>> {
        let bytes = b"mem_available_kib=2048\n";
        let archive = make_archive("velnor-guest-resource-sample", bytes)?;
        assert_eq!(extract_probe_output(archive, 512), Ok(bytes.to_vec()));
        Ok(())
    }

    #[test]
    fn oversized_archive_file_is_rejected_from_its_header() -> Result<(), Box<dyn Error>> {
        let archive = make_archive("velnor-guest-resource-sample", &vec![b'x'; 513])?;
        assert_eq!(
            extract_probe_output(archive, 512),
            Err(GuestSampleFailure::OutputLimit)
        );
        Ok(())
    }

    #[test]
    fn wrong_archive_path_or_malformed_archive_is_unknown() -> Result<(), Box<dyn Error>> {
        let wrong_path = make_archive("other", b"data")?;
        assert_eq!(
            extract_probe_output(wrong_path, 512),
            Err(GuestSampleFailure::ProbeOutput)
        );
        assert_eq!(
            extract_probe_output(b"malformed".to_vec(), 512),
            Err(GuestSampleFailure::ProbeOutput)
        );
        Ok(())
    }
}

#[cfg(all(test, unix))]
#[path = "adapter_transport_tests.rs"]
mod transport_tests;
