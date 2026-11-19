// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::plan::Action;

impl Action {
    #[must_use]
    pub fn known_inputs(self) -> &'static [&'static str] {
        match self {
            Self::Checkout => &[
                "repository",
                "ref",
                "token",
                "ssh-key",
                "ssh-known-hosts",
                "ssh-strict",
                "ssh-user",
                "persist-credentials",
                "path",
                "clean",
                "filter",
                "sparse-checkout",
                "sparse-checkout-cone-mode",
                "fetch-depth",
                "fetch-tags",
                "show-progress",
                "lfs",
                "submodules",
                "set-safe-directory",
                "github-server-url",
                "allow-unsafe-pr-checkout",
            ],
            Self::Mise => &[
                "version",
                "auto_update",
                "minimum_release_age",
                "sha256",
                "mise_dir",
                "tool_versions",
                "mise_toml",
                "install",
                "plugins",
                "install_args",
                "monorepo",
                "bootstrap",
                "bootstrap_skip",
                "bootstrap_args",
                "install_dir",
                "cache",
                "cache_save",
                "cache_save_post",
                "cache_restore_retry_budget",
                "cache_save_retry_budget",
                "cache_key_prefix",
                "cache_max_age_days",
                "cache_key",
                "experimental",
                "log_level",
                "working_directory",
                "reshim",
                "add_shims_to_path",
                "github_token",
                "persist_github_token",
                "fetch_from_github",
                "env",
                "export_path",
                "wings_enabled",
            ],
            Self::UploadArtifact => &[
                "name",
                "path",
                "if-no-files-found",
                "retention-days",
                "compression-level",
                "overwrite",
                "include-hidden-files",
                "archive",
            ],
            Self::DownloadArtifact => &[
                "name",
                "artifact-ids",
                "path",
                "pattern",
                "merge-multiple",
                "github-token",
                "repository",
                "run-id",
                "skip-decompress",
                "digest-mismatch",
            ],
            Self::CacheRestore => &[
                "path",
                "key",
                "restore-keys",
                "enableCrossOsArchive",
                "fail-on-cache-miss",
                "lookup-only",
            ],
            Self::CacheSave => &["path", "key", "upload-chunk-size", "enableCrossOsArchive"],
            Self::AttestBuildProvenance => &[
                "subject-path",
                "subject-digest",
                "subject-name",
                "subject-checksums",
                "predicate-type",
                "predicate",
                "predicate-path",
                "push-to-registry",
                "create-storage-record",
                "show-summary",
                "github-token",
            ],
            Self::GithubScript => &[
                "script",
                "github-token",
                "debug",
                "user-agent",
                "previews",
                "result-encoding",
                "retries",
                "retry-exempt-status-codes",
                "base-url",
            ],
        }
    }

    #[must_use]
    pub fn required_inputs(self) -> &'static [&'static str] {
        match self {
            Self::UploadArtifact => &["path"],
            Self::CacheRestore => &["path", "key"],
            Self::CacheSave => &["path", "key"],
            Self::GithubScript => &["script"],
            Self::Checkout | Self::Mise | Self::DownloadArtifact | Self::AttestBuildProvenance => {
                &[]
            }
        }
    }
}
