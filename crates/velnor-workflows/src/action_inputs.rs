// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::plan::Action;

const CHECKOUT: [&str; 21] = [
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
];
const MISE: [&str; 34] = [
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
];
const UPLOAD_ARTIFACT: [&str; 8] = [
    "name",
    "path",
    "if-no-files-found",
    "retention-days",
    "compression-level",
    "overwrite",
    "include-hidden-files",
    "archive",
];
const DOWNLOAD_ARTIFACT: [&str; 10] = [
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
];
const CACHE: [&str; 7] = [
    "path",
    "key",
    "restore-keys",
    "upload-chunk-size",
    "enableCrossOsArchive",
    "fail-on-cache-miss",
    "lookup-only",
];
const CACHE_RESTORE: [&str; 6] = [
    "path",
    "key",
    "restore-keys",
    "enableCrossOsArchive",
    "fail-on-cache-miss",
    "lookup-only",
];
const CACHE_SAVE: [&str; 4] = ["path", "key", "upload-chunk-size", "enableCrossOsArchive"];
const ATTEST: [&str; 11] = [
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
];
const GITHUB_SCRIPT: [&str; 9] = [
    "script",
    "github-token",
    "debug",
    "user-agent",
    "previews",
    "result-encoding",
    "retries",
    "retry-exempt-status-codes",
    "base-url",
];

const MR_BOXINGTON: [&str; 10] = [
    "backend",
    "github-cache-mode",
    "version",
    "toolchain",
    "isolate-objects-cache",
    "cache-generation",
    "cache-key-suffix",
    "save-on-pull-request",
    "save-on-protected-branch",
    "save-on-workflow-dispatch",
];

impl Action {
    #[must_use]
    pub fn known_inputs(self) -> &'static [&'static str] {
        match self {
            Self::Checkout => &CHECKOUT,
            Self::Mise => &MISE,
            Self::UploadArtifact => &UPLOAD_ARTIFACT,
            Self::DownloadArtifact => &DOWNLOAD_ARTIFACT,
            Self::Cache => &CACHE,
            Self::CacheRestore => &CACHE_RESTORE,
            Self::CacheSave => &CACHE_SAVE,
            Self::AttestBuildProvenance => &ATTEST,
            Self::GithubScript => &GITHUB_SCRIPT,
            Self::MrBoxington => &MR_BOXINGTON,
        }
    }

    #[must_use]
    pub fn required_inputs(self) -> &'static [&'static str] {
        match self {
            Self::UploadArtifact => &["path"],
            Self::Cache | Self::CacheRestore | Self::CacheSave => &["path", "key"],
            Self::GithubScript => &["script"],
            Self::Checkout
            | Self::Mise
            | Self::DownloadArtifact
            | Self::AttestBuildProvenance
            | Self::MrBoxington => &[],
        }
    }
}
