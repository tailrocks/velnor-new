// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub(crate) enum ImageMaintenanceCommand {
    /// Install the source-pinned Buildx plugin into a private runner directory.
    InstallBuildx {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        runner_temp: PathBuf,
    },
    /// Verify exact checkout identity and write native source-closure evidence.
    CheckJobSource {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        source_commit: String,
        #[arg(long)]
        source_closure_path: PathBuf,
    },
    /// Build and validate an isolated OCI candidate with immutable source evidence.
    BuildJobCandidate {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        source_commit: String,
        #[arg(long)]
        runner_temp: PathBuf,
        #[arg(long)]
        archive_path: PathBuf,
        #[arg(long)]
        record_path: PathBuf,
        #[arg(long)]
        source_closure_path: PathBuf,
    },
    /// Verify downloaded candidate bytes against their exact source closure.
    ValidateJobCandidate {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        archive_path: PathBuf,
        #[arg(long)]
        record_path: PathBuf,
        #[arg(long)]
        source_closure_path: PathBuf,
    },
}
