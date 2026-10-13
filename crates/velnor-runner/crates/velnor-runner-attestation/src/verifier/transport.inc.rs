#[derive(Clone, Debug)]
struct BoundedTufTransport {
    client: Client,
    capture: RootResponseCapture,
}

impl BoundedTufTransport {
    fn new(capture: RootResponseCapture) -> Result<Self, Box<dyn Error>> {
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { client, capture })
    }

    fn allowed_url(url: &Url) -> bool {
        url.scheme() == "https"
            && url.host_str() == Some("tuf-repo-cdn.sigstore.dev")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && !url.path().contains("..")
    }

    fn response_size_allowed(current: usize, next: usize) -> bool {
        current.saturating_add(next) <= MAX_TUF_RESPONSE_BYTES
    }
}

#[async_trait]
impl Transport for BoundedTufTransport {
    async fn fetch(&self, url: Url) -> Result<TransportStream, TransportError> {
        let safe_url = "Sigstore public-good TUF endpoint";
        if !Self::allowed_url(&url) {
            return Err(TransportError::new(
                TransportErrorKind::UnsupportedUrlScheme,
                safe_url,
            ));
        }
        let response = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| TransportError::new(TransportErrorKind::Other, safe_url))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(TransportError::new(
                TransportErrorKind::FileNotFound,
                safe_url,
            ));
        }
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > MAX_TUF_RESPONSE_BYTES as u64)
        {
            return Err(TransportError::new(TransportErrorKind::Other, safe_url));
        }
        let mut body = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks
            .try_next()
            .await
            .map_err(|_| TransportError::new(TransportErrorKind::Other, safe_url))?
        {
            if !Self::response_size_allowed(body.len(), chunk.len()) {
                return Err(TransportError::new(TransportErrorKind::Other, safe_url));
            }
            body.extend_from_slice(&chunk);
        }
        self.capture
            .record(&url, &body)
            .map_err(|_| TransportError::new(TransportErrorKind::Other, safe_url))?;
        Ok(Box::pin(stream::iter([Ok(Bytes::from(body))])))
    }
}

async fn load_public_good_trusted_root() -> Result<TrustedRoot, Box<dyn Error>> {
    let base = Url::parse("https://tuf-repo-cdn.sigstore.dev/")?;
    let target = TargetName::new("trusted_root.json")?;
    let capture = RootResponseCapture::new();
    let refreshed = TufCache::new(tuf_cache_path()?)
        .refresh(TufRefreshRequest {
            bootstrap: SIGSTORE_TUF_ROOT,
            migration: None,
            metadata_url: base.clone(),
            targets_url: base.join("targets/")?,
            target_name: Some(&target),
            transport: BoundedTufTransport::new(capture.clone())?,
            capture: &capture,
            validate_target: |bytes: Option<&[u8]>| {
                let bytes = bytes.ok_or_else(|| -> Box<dyn Error> {
                    "authenticated trusted root target is missing".into()
                })?;
                Ok(TrustedRoot::from_json(std::str::from_utf8(bytes)?)?)
            },
        })
        .await?;
    Ok(refreshed.target_value)
}

fn tuf_cache_path() -> Result<std::path::PathBuf, Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or("installed helper path has no parent")?;
    Ok(directory.join("attestation-tuf-cache"))
}
