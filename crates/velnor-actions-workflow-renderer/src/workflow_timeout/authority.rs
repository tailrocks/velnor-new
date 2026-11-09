// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{invalid, lower_hex};
use crate::RenderError;
use std::collections::BTreeSet;
use velnor_actions_contract::JobTimeout;

const MAC_PROOF_AUTHORITY_SHA: &str = "533d225c08785adc4367340e1db4dc111ee2b5a2";

pub(super) fn admits(path: &str, id: &str, value: &str) -> Result<bool, RenderError> {
    if path == ".github/workflows/product-release-generator.yml"
        && id == "source-preview-publication"
        && value
            == format!(
                "tailrocks/velnor-new/.github/workflows/source-preview-publication.yml@{SOURCE_PREVIEW_AUTHORITY_SHA}"
            )
    {
        validate_source_preview_authority()?;
        return Ok(true);
    }
    if path == ".github/workflows/product-release-images.yml"
        && id == "verify-mac-proof"
        && velnor_actions_contract::source_preview::RUNTIME_AUTHORITY_SHA
            == Some(MAC_PROOF_AUTHORITY_SHA)
        && value
            == format!(
                "tailrocks/velnor-new/.github/workflows/source-preview-mac-proof.yml@{MAC_PROOF_AUTHORITY_SHA}"
            )
    {
        validate_authority_graph(
            "source-preview-mac-proof",
            MAC_PROOF_AUTHORITY_SHA,
            "ff3ef47eb0f5ea6e43eedb505c5aea15a1ba2e8f523d81959f81e592034b3f1c",
            &[AuthorityJob {
                id: "verify",
                minutes: 140,
                needs: &[],
            }],
        )?;
        return Ok(true);
    }
    Ok(false)
}

const SOURCE_PREVIEW_AUTHORITY_SHA: &str =
    velnor_actions_contract::source_preview::PUBLICATION_AUTHORITY_SHA;

struct AuthorityJob {
    id: &'static str,
    minutes: u32,
    needs: &'static [&'static str],
}

fn validate_source_preview_authority() -> Result<(), RenderError> {
    const BLOB_SHA256: &str = "b157479660663d7241291be5f392f4338e6b60a2063bda61d7aa02f82f94990e";
    const JOBS: &[AuthorityJob] = &[
        AuthorityJob {
            id: "source-preview-source",
            minutes: 5,
            needs: &[],
        },
        AuthorityJob {
            id: "source-preview-draft",
            minutes: 10,
            needs: &["source-preview-source"],
        },
        AuthorityJob {
            id: "source-preview-attest",
            minutes: 10,
            needs: &["source-preview-source", "source-preview-draft"],
        },
        AuthorityJob {
            id: "source-preview-publish",
            minutes: 10,
            needs: &[
                "source-preview-source",
                "source-preview-draft",
                "source-preview-attest",
            ],
        },
    ];
    validate_authority_graph(
        "source-preview-publication",
        SOURCE_PREVIEW_AUTHORITY_SHA,
        BLOB_SHA256,
        JOBS,
    )
}

fn validate_authority_graph(
    id: &str,
    sha: &str,
    blob: &str,
    jobs: &[AuthorityJob],
) -> Result<(), RenderError> {
    if !lower_hex(blob, 64) || !lower_hex(sha, 40) {
        return Err(invalid(id, "invalid_authority_identity"));
    }
    let mut seen = BTreeSet::new();
    for job in jobs {
        JobTimeout::try_from(job.minutes).map_err(RenderError::Contract)?;
        if job
            .needs
            .iter()
            .any(|dependency| !seen.contains(dependency))
            || !seen.insert(job.id)
        {
            return Err(invalid(id, "invalid_authority_graph"));
        }
    }
    Ok(())
}
