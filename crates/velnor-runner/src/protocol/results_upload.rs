// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result, bail};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use std::path::Path;
use std::time::Duration;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use url::Url;

use super::error::sanitize_response_body;
use super::http::{http_client, redacted_reqwest_error, validate_authenticated_url};
use super::results::{BlobKind, STEP_SUMMARY, TwirpResultsClient, retrying};
use super::results_wire::{
    MetadataRequest, ScopeRequest, SignedUrlResponse, SoftSizeLimitExceeded,
};

pub(super) const BLOCK_SIZE: usize = 2 * 1024 * 1024;
const BLOB_TYPE: &str = "x-ms-blob-type";
const BLOB_SEALED: &str = "x-ms-blob-sealed";

struct LogBlocks {
    reader: BufReader<File>,
    lines: i64,
}

impl LogBlocks {
    async fn open(path: &Path) -> Result<(Self, u64)> {
        let file = File::open(path)
            .await
            .with_context(|| format!("open {}", path.display()))?;
        let size = file
            .metadata()
            .await
            .with_context(|| format!("stat {}", path.display()))?
            .len();
        Ok((
            Self {
                reader: BufReader::new(file),
                lines: 0,
            },
            size,
        ))
    }

    async fn next(&mut self) -> Result<Option<(Vec<u8>, bool)>> {
        let mut block = Vec::with_capacity(BLOCK_SIZE);
        while block.len() < BLOCK_SIZE {
            if self.reader.read_until(b'\n', &mut block).await? == 0 {
                break;
            }
            self.lines += 1;
        }
        if block.is_empty() {
            return Ok(None);
        }
        let last = self.reader.fill_buf().await?.is_empty();
        Ok(Some((block, last)))
    }
}

fn append_url(url: &Url, seal: bool) -> Url {
    let mut url = url.clone();
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("comp", "appendblock");
        if seal {
            query.append_pair("seal", "true");
        }
    }
    url
}

async fn put(
    url: &Url,
    label: &str,
    budget: Duration,
    body: Vec<u8>,
    headers: &[(&str, &str)],
) -> Result<()> {
    let empty = body.is_empty();
    let mut request = http_client()?
        .put(url.clone())
        .header(CONTENT_TYPE, "text/plain")
        .timeout(budget)
        .body(body);
    if empty {
        request = request.header(CONTENT_LENGTH, 0);
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("{label} PUT: {}", redacted_reqwest_error(&error)))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!(
            "{label} PUT failed: status={status}, body={}",
            sanitize_response_body(&body)
        );
    }
    Ok(())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

impl TwirpResultsClient {
    async fn signed(&self, kind: BlobKind, scope: &ScopeRequest<'_>) -> Result<SignedUrlResponse> {
        let body = serde_json::to_string(scope)
            .with_context(|| format!("serialize {}", kind.signed_url_method))?;
        let text = self
            .twirp_post(
                &self.receiver_url(kind.signed_url_method),
                &body,
                kind.signed_url_method,
            )
            .await?;
        serde_json::from_str(&text).with_context(|| format!("parse {}", kind.signed_url_method))
    }

    fn blob_url(kind: BlobKind, response: SignedUrlResponse) -> Result<Url> {
        let url = (kind.signed_url)(response)
            .filter(|url| !url.is_empty())
            .with_context(|| format!("{} returned an empty URL", kind.signed_url_method))?;
        validate_authenticated_url(&url)
            .with_context(|| format!("validate {} signed blob URL", kind.label))
    }

    async fn create_metadata(
        &self,
        kind: BlobKind,
        scope: &ScopeRequest<'_>,
        line_count: Option<i64>,
        size: Option<i64>,
    ) -> Result<()> {
        let body = serde_json::to_string(&MetadataRequest {
            scope,
            uploaded_at: now_rfc3339(),
            line_count,
            size,
        })
        .with_context(|| format!("serialize {}", kind.metadata_method))?;
        self.twirp_acknowledged(
            &self.receiver_url(kind.metadata_method),
            &body,
            kind.metadata_method,
        )
        .await
    }

    pub(super) async fn upload_log(
        &self,
        kind: BlobKind,
        scope: &ScopeRequest<'_>,
        path: &Path,
    ) -> Result<()> {
        retrying(kind.label, || async {
            let (mut blocks, size) = LogBlocks::open(path).await?;
            let url = Self::blob_url(kind, self.signed(kind, scope).await?)?;
            let (label, budget) = (kind.label, self.block_budget);
            if size <= BLOCK_SIZE as u64 {
                let (content, _) = blocks.next().await?.unwrap_or_default();
                put(&url, label, budget, content, &[(BLOB_TYPE, "BlockBlob")]).await?;
            } else {
                put(
                    &url,
                    label,
                    budget,
                    Vec::new(),
                    &[(BLOB_TYPE, "AppendBlob")],
                )
                .await?;
                let (append, seal) = (append_url(&url, false), append_url(&url, true));
                while let Some((block, last)) = blocks.next().await? {
                    let (target, sealed) = if last {
                        (&seal, "True")
                    } else {
                        (&append, "False")
                    };
                    put(target, label, budget, block, &[(BLOB_SEALED, sealed)]).await?;
                }
            }
            self.create_metadata(kind, scope, Some(blocks.lines), None)
                .await
        })
        .await
    }

    pub(super) async fn upload_summary(&self, scope: &ScopeRequest<'_>, path: &Path) -> Result<()> {
        let size = i64::try_from(
            tokio::fs::metadata(path)
                .await
                .with_context(|| format!("stat {}", path.display()))?
                .len(),
        )
        .context("step summary size")?;
        let kind = STEP_SUMMARY;
        retrying(kind.label, || async {
            let signed = self.signed(kind, scope).await?;
            if let Some(limit) = signed.soft_size_limit
                && size > limit
            {
                return Err(SoftSizeLimitExceeded {
                    label: kind.label,
                    size,
                    limit,
                }
                .into());
            }
            let url = Self::blob_url(kind, signed)?;
            let content = tokio::fs::read(path)
                .await
                .with_context(|| format!("read {}", path.display()))?;
            let headers = [(BLOB_TYPE, "BlockBlob")];
            put(&url, kind.label, self.block_budget, content, &headers).await?;
            self.create_metadata(kind, scope, None, Some(size)).await
        })
        .await
    }
}
