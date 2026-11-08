// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use clap::ValueEnum;
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
    #[command(
        about = "Inspect exact-source Runner, DinD, and resource-probe OCI archives and write descriptors."
    )]
    AdmitRunnerImages {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        source_commit: String,
        #[arg(long)]
        runner_archive: PathBuf,
        #[arg(long)]
        dind_archive: PathBuf,
        #[arg(long)]
        probe_archive: PathBuf,
        #[arg(long)]
        descriptors_output: PathBuf,
    },
    #[command(about = "Build and project the runner family from held context snapshots.")]
    BuildRunnerImages {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        source_commit: String,
        #[arg(long)]
        runner_temp: PathBuf,
        #[arg(long)]
        contexts_dir: PathBuf,
        #[arg(long)]
        publication_records: bool,
        #[arg(
            long,
            help = "Fresh output directory under an existing private parent."
        )]
        output_dir: PathBuf,
    },
    #[command(about = "Publish a qualified runner family and write its manifest.")]
    PublishRunnerImages(RunnerFamilyPublicationArgs),
    #[command(about = "Recheck runner publication authority and registry identities.")]
    VerifyRunnerPublication(RunnerFamilyPublicationArgs),
    /// Promote a verified source digest to its approved version tag.
    PromoteImage {
        #[arg(long)]
        image_id: String,
        #[arg(long)]
        source_commit: String,
    },
    /// Verify publication custody and publish one admitted image by digest.
    PublishImage {
        #[arg(long, value_enum)]
        profile: ImagePublicationProfileArg,
        #[arg(long)]
        source_commit: String,
        #[arg(long)]
        producer_run_id: u64,
        #[arg(long)]
        producer_run_attempt: u32,
        #[arg(long)]
        artifact_id: u64,
        #[arg(long)]
        archive_path: PathBuf,
        #[arg(long)]
        record_path: PathBuf,
        #[arg(long)]
        qualification_proof: Option<PathBuf>,
        #[arg(long)]
        job_attestation_run_id: Option<u64>,
        #[arg(long)]
        job_attestation_run_attempt: Option<u32>,
        #[arg(long)]
        private_scratch: PathBuf,
        #[arg(long)]
        receipt_output: PathBuf,
    },
}

#[derive(Debug, clap::Args)]
pub(crate) struct RunnerFamilyPublicationArgs {
    #[arg(long)]
    pub(crate) source_commit: String,
    #[arg(long)]
    pub(crate) producer_run_id: u64,
    #[arg(long)]
    pub(crate) producer_run_attempt: u32,
    #[arg(long)]
    pub(crate) artifact_id: u64,
    #[arg(long)]
    pub(crate) assets_dir: PathBuf,
    #[arg(long)]
    pub(crate) qualification_proof: PathBuf,
    #[arg(long)]
    pub(crate) private_scratch: PathBuf,
    #[arg(long)]
    pub(crate) manifest_output: PathBuf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum ImagePublicationProfileArg {
    Job,
    Runner,
    Dind,
    ResourceProbe,
}
