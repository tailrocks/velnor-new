//! Immutable identities returned only after release and selected-daemon verification.

/// Verified runner and DinD images prepared for one selected Docker engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedWorkerImages {
    engine_id: String,
    docker_root_digest: String,
    daemon_architecture: String,
    source_sha: String,
    platform: String,
    runner: VerifiedImageIdentity,
    dind: VerifiedImageIdentity,
}

impl VerifiedWorkerImages {
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn daemon_architecture(&self) -> &str {
        &self.daemon_architecture
    }

    pub(crate) fn source_sha(&self) -> &str {
        &self.source_sha
    }

    pub(crate) fn platform(&self) -> &str {
        &self.platform
    }

    pub(crate) fn runner(&self) -> &VerifiedImageIdentity {
        &self.runner
    }

    pub(crate) fn dind(&self) -> &VerifiedImageIdentity {
        &self.dind
    }
}

/// Archive and selected-image identities for one verified worker image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedImageIdentity {
    runtime_id: String,
    archive_sha256: String,
    archive_index_digest: String,
    image_index_digest: Option<String>,
    manifest_digest: String,
    config_digest: String,
}

impl VerifiedImageIdentity {
    pub(crate) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub(crate) fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    /// Digest of the outer archive's `index.json` bytes.
    pub(crate) fn archive_index_digest(&self) -> &str {
        &self.archive_index_digest
    }

    /// Digest of the selected nested OCI index, when the archive contains one.
    pub(crate) fn image_index_digest(&self) -> Option<&str> {
        self.image_index_digest.as_deref()
    }

    pub(crate) fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub(crate) fn config_digest(&self) -> &str {
        &self.config_digest
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestWorkerImageMutation {
    RunnerRuntimeId,
    RunnerIndexDigest,
    RunnerManifestDigest,
    RunnerConfigDigest,
    Platform,
    SourceSha,
}

#[cfg(test)]
pub(crate) fn test_worker_images() -> VerifiedWorkerImages {
    test_worker_images_with(None)
}

#[cfg(test)]
pub(crate) fn test_worker_images_with(
    mutation: Option<TestWorkerImageMutation>,
) -> VerifiedWorkerImages {
    let mut images = VerifiedWorkerImages {
        engine_id: "selected-engine".to_owned(),
        docker_root_digest: "0".repeat(64),
        daemon_architecture: "aarch64".to_owned(),
        source_sha: "a".repeat(40),
        platform: "linux/amd64".to_owned(),
        runner: test_image("d", "e", Some("f"), "a", "b"),
        dind: test_image("8", "7", None, "6", "5"),
    };
    match mutation {
        Some(TestWorkerImageMutation::RunnerRuntimeId) => {
            images.runner.runtime_id = "mutable:tag".to_owned();
        }
        Some(TestWorkerImageMutation::RunnerIndexDigest) => {
            images.runner.image_index_digest = Some("not-a-digest".to_owned());
        }
        Some(TestWorkerImageMutation::RunnerManifestDigest) => {
            images.runner.manifest_digest = "not-a-digest".to_owned();
        }
        Some(TestWorkerImageMutation::RunnerConfigDigest) => {
            images.runner.config_digest = "not-a-digest".to_owned();
        }
        Some(TestWorkerImageMutation::Platform) => {
            images.platform = "linux/arm64".to_owned();
        }
        Some(TestWorkerImageMutation::SourceSha) => {
            images.source_sha = "not-a-sha".to_owned();
        }
        None => {}
    }
    images
}

#[cfg(test)]
fn test_image(
    archive_nibble: &str,
    archive_index_nibble: &str,
    image_index_nibble: Option<&str>,
    manifest_nibble: &str,
    config_nibble: &str,
) -> VerifiedImageIdentity {
    let runtime_id = image_index_nibble
        .map(|nibble| format!("sha256:{}", nibble.repeat(64)))
        .unwrap_or_else(|| format!("sha256:{}", config_nibble.repeat(64)));
    VerifiedImageIdentity {
        runtime_id,
        archive_sha256: archive_nibble.repeat(64),
        archive_index_digest: archive_index_nibble.repeat(64),
        image_index_digest: image_index_nibble
            .map(|nibble| format!("sha256:{}", nibble.repeat(64))),
        manifest_digest: format!("sha256:{}", manifest_nibble.repeat(64)),
        config_digest: format!("sha256:{}", config_nibble.repeat(64)),
    }
}
