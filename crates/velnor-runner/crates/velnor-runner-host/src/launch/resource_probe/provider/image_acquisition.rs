// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::models::{ImageInspect, SystemInfo};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, body::Body, client::conn::http1};
use hyper_util::rt::TokioIo;
use velnor_runner_image_manifest::{ImageKind, strict_json};

use super::identity::{SelectedDaemon, selected_daemon};
use crate::error::HostError;
#[path = "raw_digest_pull/decoder.rs"]
mod decoder;
use decoder::PullDecoder;

const MAX_BODY: usize = 4 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const PULL_TIMEOUT: Duration = Duration::from_mins(30);

#[derive(Debug)]
pub struct DaemonSelection {
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) architecture: String,
}

impl DaemonSelection {
    pub fn new(
        engine_id: &str,
        docker_root_digest: &str,
        architecture: &str,
    ) -> Result<Self, HostError> {
        if engine_id.trim().is_empty()
            || !super::valid_hex(docker_root_digest, 64)
            || !matches!(architecture, "x86_64" | "aarch64")
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            engine_id: engine_id.to_owned(),
            docker_root_digest: docker_root_digest.to_owned(),
            architecture: architecture.to_owned(),
        })
    }
}

#[derive(Debug)]
pub struct ImageReference {
    role: ImageKind,
    digest: String,
}

impl ImageReference {
    pub fn new(role: ImageKind, digest: &str) -> Result<Self, HostError> {
        if !digest
            .strip_prefix("sha256:")
            .is_some_and(|value| super::valid_hex(value, 64))
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            role,
            digest: digest.to_owned(),
        })
    }

    fn reference(&self) -> String {
        format!("{}@{}", self.role.repository(), self.digest)
    }
}

pub struct ImageObservation {
    pub(super) inspect: ImageInspect,
    pub(super) store: ImageStore,
    pub(super) live: SelectedDaemon,
}

impl std::fmt::Debug for ImageObservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ImageObservation")
            .field("runtime_id", &self.runtime_id())
            .finish_non_exhaustive()
    }
}

impl ImageObservation {
    #[must_use]
    pub fn runtime_id(&self) -> Option<&str> {
        self.inspect.id.as_deref()
    }
    #[must_use]
    pub fn repository_digests(&self) -> &[String] {
        self.inspect.repo_digests.as_deref().unwrap_or_default()
    }
    #[must_use]
    pub fn os(&self) -> Option<&str> {
        self.inspect.os.as_deref()
    }
    #[must_use]
    pub fn architecture(&self) -> Option<&str> {
        self.inspect.architecture.as_deref()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ImageStore {
    Classic,
    Containerd,
}

pub struct ImageSession {
    sender: http1::SendRequest<Full<Bytes>>,
    task: tokio::task::JoinHandle<()>,
    expected: DaemonSelection,
    failed: bool,
    store: Option<ImageStore>,
}

impl std::fmt::Debug for ImageSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ImageSession")
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

impl Drop for ImageSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ImageSession {
    pub async fn connect(endpoint: &str, expected: &DaemonSelection) -> Result<Self, HostError> {
        let expected = DaemonSelection::new(
            &expected.engine_id,
            &expected.docker_root_digest,
            &expected.architecture,
        )?;
        let socket = endpoint.strip_prefix("unix://").unwrap_or(endpoint);
        if !socket.starts_with('/') || socket.len() <= 1 || socket.chars().any(char::is_control) {
            return Err(HostError::Identity);
        }
        let stream = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::UnixStream::connect(socket),
        )
        .await
        .map_err(|_| HostError::Docker)?
        .map_err(|_| HostError::Docker)?;
        let (sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| HostError::Docker)?;
        let task = tokio::spawn(async move {
            drop(connection.await);
        });
        let mut session = Self {
            sender,
            task,
            expected,
            failed: false,
            store: None,
        };
        session.info().await?;
        Ok(session)
    }

    pub async fn inspect(
        &mut self,
        image: &ImageReference,
    ) -> Result<Option<ImageObservation>, HostError> {
        if self.failed {
            return Err(HostError::Docker);
        }
        self.failed = true;
        let result = self.inspect_inner(image).await;
        self.failed = result.is_err();
        result
    }

    pub async fn pull_and_inspect(
        &mut self,
        image: &ImageReference,
    ) -> Result<ImageObservation, HostError> {
        if self.failed {
            return Err(HostError::Docker);
        }
        self.failed = true;
        let result = self.pull_inner(image).await;
        self.failed = result.is_err();
        result
    }

    async fn inspect_inner(
        &mut self,
        image: &ImageReference,
    ) -> Result<Option<ImageObservation>, HostError> {
        let (_, store) = self.info().await?;
        let path = format!(
            "/v{}.{}/images/{}/json?platform=%7B%22architecture%22%3A%22amd64%22%2C%22os%22%3A%22linux%22%7D",
            bollard::API_DEFAULT_VERSION.major_version,
            bollard::API_DEFAULT_VERSION.minor_version,
            image.reference()
        );
        let (status, bytes) = self.request(Method::GET, &path, REQUEST_TIMEOUT).await?;
        if status == hyper::StatusCode::NOT_FOUND {
            self.info().await?;
            return Ok(None);
        }
        if !status.is_success() {
            return Err(HostError::Docker);
        }
        let value = strict_json::parse_strict_json_bytes(&bytes, MAX_BODY + 1)
            .map_err(|_| HostError::Identity)?;
        let inspect = serde_json::from_value(value).map_err(|_| HostError::Identity)?;
        let (live, after) = self.info().await?;
        if store != after {
            return Err(HostError::Identity);
        }
        Ok(Some(ImageObservation {
            inspect,
            store,
            live,
        }))
    }

    async fn pull_inner(&mut self, image: &ImageReference) -> Result<ImageObservation, HostError> {
        let mut url = reqwest::Url::parse(&format!(
            "http://localhost/v{}.{}/images/create",
            bollard::API_DEFAULT_VERSION.major_version,
            bollard::API_DEFAULT_VERSION.minor_version
        ))
        .map_err(|_| HostError::Identity)?;
        url.query_pairs_mut()
            .append_pair("fromImage", image.role.repository())
            .append_pair("tag", &image.digest)
            .append_pair("platform", "linux/amd64");
        let path = format!("{}?{}", url.path(), url.query().ok_or(HostError::Identity)?);
        let (status, bytes) = self.request(Method::POST, &path, PULL_TIMEOUT).await?;
        if !status.is_success() {
            return Err(HostError::Docker);
        }
        let reference = image.reference();
        let mut decoder = PullDecoder::new(&image.digest, &reference);
        decoder.push(&bytes).map_err(|_| HostError::Docker)?;
        decoder.finish().map_err(|_| HostError::Docker)?;
        self.inspect_inner(image).await?.ok_or(HostError::Identity)
    }

    async fn info(&mut self) -> Result<(SelectedDaemon, ImageStore), HostError> {
        let (status, bytes) = self
            .request(
                Method::GET,
                &format!(
                    "/v{}.{}/info",
                    bollard::API_DEFAULT_VERSION.major_version,
                    bollard::API_DEFAULT_VERSION.minor_version
                ),
                REQUEST_TIMEOUT,
            )
            .await?;
        if !status.is_success() {
            return Err(HostError::Docker);
        }
        let value = strict_json::parse_strict_json_bytes(&bytes, MAX_BODY + 1)
            .map_err(|_| HostError::Identity)?;
        let info: SystemInfo = serde_json::from_value(value).map_err(|_| HostError::Identity)?;
        let current =
            selected_daemon(&info, &self.expected.engine_id).map_err(|_| HostError::Identity)?;
        if current.docker_root_digest != self.expected.docker_root_digest
            || current.architecture != self.expected.architecture
        {
            return Err(HostError::Identity);
        }
        let mut drivers = info
            .driver_status
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|entry| entry.first().is_some_and(|key| key == "driver-type"));
        let store = match drivers.next() {
            None => ImageStore::Classic,
            Some(entry) if entry.len() == 2 && entry[1] == "io.containerd.snapshotter.v1" => {
                ImageStore::Containerd
            }
            _ => return Err(HostError::Identity),
        };
        if drivers.next().is_some() || self.store.is_some_and(|initial| initial != store) {
            return Err(HostError::Identity);
        }
        self.store = Some(store);
        Ok((current, store))
    }

    async fn request(
        &mut self,
        method: Method,
        path: &str,
        timeout: Duration,
    ) -> Result<(hyper::StatusCode, Vec<u8>), HostError> {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost")
            .body(Full::<Bytes>::default())
            .map_err(|_| HostError::Docker)?;
        tokio::time::timeout(timeout, async {
            let response = self
                .sender
                .send_request(request)
                .await
                .map_err(|_| HostError::Docker)?;
            let status = response.status();
            if response
                .body()
                .size_hint()
                .upper()
                .is_some_and(|size| size > MAX_BODY as u64)
            {
                return Err(HostError::Frame);
            }
            let mut body = response.into_body();
            let mut bytes = Vec::new();
            while let Some(frame) = body.frame().await {
                let frame = frame.map_err(|_| HostError::Docker)?;
                if let Some(data) = frame.data_ref() {
                    if bytes
                        .len()
                        .checked_add(data.len())
                        .is_none_or(|size| size > MAX_BODY)
                    {
                        return Err(HostError::Frame);
                    }
                    bytes.extend_from_slice(data);
                }
            }
            Ok((status, bytes))
        })
        .await
        .map_err(|_| HostError::Docker)?
    }
}
