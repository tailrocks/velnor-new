// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FixtureProfile {
    Job,
    Runner,
    Dind,
    ResourceProbe,
}

impl FixtureProfile {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Job => "job",
            Self::Runner => "runner",
            Self::Dind => "dind",
            Self::ResourceProbe => "resource_probe",
        }
    }

    pub(crate) const fn cli_name(self) -> &'static str {
        match self {
            Self::Job => "job",
            Self::Runner => "runner",
            Self::Dind => "dind",
            Self::ResourceProbe => "resource-probe",
        }
    }

    pub(crate) const fn repository(self) -> &'static str {
        match self {
            Self::Job => "ghcr.io/tailrocks/velnor-job",
            Self::Runner => "ghcr.io/tailrocks/velnor-runner",
            Self::Dind => "ghcr.io/tailrocks/velnor-dind",
            Self::ResourceProbe => "ghcr.io/tailrocks/velnor-resource-probe",
        }
    }

    pub(crate) const fn archive_name(self) -> &'static str {
        match self {
            Self::Job => "job-image.oci.tar",
            Self::Runner => "velnor-runner-linux-amd64.tar",
            Self::Dind => "velnor-dind-linux-amd64.tar",
            Self::ResourceProbe => "velnor-resource-probe-linux-amd64.tar",
        }
    }

    pub(crate) const fn record_name(self) -> &'static str {
        match self {
            Self::Job => "job-image-candidate.json",
            Self::Runner => "runner-image-publication.json",
            Self::Dind => "dind-image-publication.json",
            Self::ResourceProbe => "resource-probe-image-publication.json",
        }
    }

    pub(crate) const fn envelope_name(self) -> &'static str {
        match self {
            Self::Job => "",
            Self::Runner => "runner-image-provenance.json",
            Self::Dind => "dind-image-provenance.json",
            Self::ResourceProbe => "resource-probe-image-provenance.json",
        }
    }

    pub(crate) const fn run_id(self) -> u64 {
        match self {
            Self::Job => 40_613_392_798,
            Self::Runner | Self::Dind | Self::ResourceProbe => 40_613_392_799,
        }
    }

    pub(crate) const fn artifact_id(self) -> u64 {
        match self {
            Self::Job => 987_654_321,
            Self::Runner | Self::Dind | Self::ResourceProbe => 987_654_322,
        }
    }

    pub(crate) const fn ci_run_id(self) -> u64 {
        match self {
            Self::Job => 40_613_400_001,
            Self::Runner | Self::Dind | Self::ResourceProbe => self.run_id(),
        }
    }

    pub(crate) const fn qualification_run_id(self) -> u64 {
        match self {
            Self::Job => 40_613_410_001,
            Self::Runner | Self::Dind | Self::ResourceProbe => 40_613_410_002,
        }
    }

    pub(crate) const fn publisher_run_id(self) -> u64 {
        match self {
            Self::Job => 40_613_420_001,
            Self::Runner | Self::Dind | Self::ResourceProbe => 40_613_420_002,
        }
    }

    pub(crate) const fn qualification_job_id(self) -> u64 {
        match self {
            Self::Job => 761_239_010,
            Self::Runner | Self::Dind | Self::ResourceProbe => 761_239_011,
        }
    }

    pub(crate) const fn job_attestor_run_id(self) -> u64 {
        match self {
            Self::Job => self.run_id(),
            Self::Runner | Self::Dind | Self::ResourceProbe => 0,
        }
    }

    pub(crate) const fn workflow_path(self) -> &'static str {
        match self {
            Self::Job => ".github/workflows/job-image-candidate.yml",
            Self::Runner | Self::Dind | Self::ResourceProbe => ".github/workflows/ci.yml",
        }
    }

    pub(crate) const fn publication_workflow_path(self) -> &'static str {
        match self {
            Self::Job => ".github/workflows/publish.yml",
            Self::Runner | Self::Dind | Self::ResourceProbe => {
                ".github/workflows/product-release-images.yml"
            }
        }
    }

    pub(crate) fn current_main_sha(self, source: &str) -> String {
        match self {
            Self::Job => source.to_owned(),
            Self::Runner | Self::Dind | Self::ResourceProbe => "b".repeat(40),
        }
    }
}
