//! Private publication capability. It grants source rendering, never SDK trust.

use super::{invalid, source_sha256};
use crate::OrchestratorError;

/// Independently reviewed authenticated source publication, never SDK trust.
/// Runtime JSON never supplies this record.
#[derive(Clone, PartialEq, Eq)]
struct SourcePublicationTuple {
    repository: &'static str,
    repository_id: u64,
    source_ref: &'static str,
    commit: &'static str,
    tree: &'static str,
    parent: &'static str,
    source_files: usize,
    source_manifest_sha256: &'static str,
    action_path: &'static str,
    action_blob: &'static str,
    action_sha256: &'static str,
    template_blob: &'static str,
    template_sha256: &'static str,
    renderer_blob: &'static str,
    renderer_sha256: &'static str,
    api_receipt_sha256: &'static str,
    independent_review_sha256: &'static str,
}

const REVIEWED_SOURCE: SourcePublicationTuple = SourcePublicationTuple {
    repository: "tailrocks/velnor-new",
    repository_id: 1_390_620_900,
    source_ref: "refs/heads/owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf",
    commit: "8758d976a1b25eb387f48aa04ea86f57739b84cf",
    tree: "e5996c377b521e75f41eeb4950623b325295d6e2",
    parent: "55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
    source_files: 190,
    // Sorted path records with blob_sha1/bytes/mode/path/sha256; compact,
    // sorted-key UTF-8 JSON without a trailing newline. Bound by API receipt.
    source_manifest_sha256: "b12cc179989794bfd2b4c1df44d1d2e3a9c64a4ab9ab211f93558a92612696fa",
    action_path: "foundation-qualification",
    action_blob: "d2e767b6ab05ee9e75e19e75d76bfc74c1fc5721",
    action_sha256: "910201ee1ab9b45ca78a05f1440620618828683620409e3d9611120fd3e942da",
    template_blob: "e243dc6e154fa5d030aa9654e616b2e221acad32",
    template_sha256: "01acba73d0dd12facb25f0985e9141ab16f6fb8941fdc72f54aa10f31914b6a7",
    renderer_blob: "5dd46e96e34fd2d854851c9207aad08013ef7ff6",
    renderer_sha256: "02ae39e609990b71e918c45832f740182a23cbc49a67cb430009d6afd8da1bb0",
    api_receipt_sha256: "8330407c2b74b9e82c4e9227e3797af71a1c8f02c9842d4c07ca34a91f2d494b",
    independent_review_sha256: "2c9f539f780e030aafe0587786988495f2d88df899091670d0e015e4206c8a85",
};

// Exact authenticated action blob. This source identity fixes Node 24 and the
// entrypoint; it is not an ambient runner or native compatibility observation.
const ACTION_DESCRIPTOR: &str = r"name: Foundation native qualification
description: First-step fresh-runner SDK observation and native positive probes
outputs:
  receipt-path:
    description: Exact native-positive receipt path
  receipt-sha256:
    description: SHA-256 of exact native-positive receipt bytes
  observation-path:
    description: Complete SDK closure observation path
  observation-sha256:
    description: SHA-256 of exact SDK observation bytes
  control-root:
    description: Independent private control root, outside payload namespaces
  fresh-gh-receipt-path:
    description: Exact sealed fresh GH native positive receipt path
  fresh-gh-receipt-sha256:
    description: SHA-256 of exact sealed fresh GH receipt bytes
runs:
  using: node24
  main: ../owned-cache/foundation-native-entrypoint.mjs
";

/// Closed equality checks the actual publication tuple, never hash syntax.
fn validate_tuple(tuple: &SourcePublicationTuple) -> Result<(), OrchestratorError> {
    if tuple != &REVIEWED_SOURCE
        || source_sha256(ACTION_DESCRIPTOR.as_bytes()) != tuple.action_sha256
    {
        return Err(invalid("unreviewed_source_publication"));
    }
    Ok(())
}

/// This module alone may issue an immutable reviewed action reference.
/// There is deliberately no caller JSON loader or public constructor.
pub(super) struct PublishedFoundationSource {
    action_reference: String,
    _publication: &'static SourcePublicationTuple,
}

impl PublishedFoundationSource {
    pub(super) fn action_reference(&self) -> &str {
        &self.action_reference
    }
}

/// Issue only the closed source tuple checked against actual publication proof.
/// This permits rendering the qualification workflow; it cannot issue any
/// runtime Foundation, SDK, native-host, cache producer or release capability.
pub(super) fn published_foundation_source() -> Result<PublishedFoundationSource, OrchestratorError>
{
    validate_tuple(&REVIEWED_SOURCE)?;
    Ok(PublishedFoundationSource {
        action_reference: format!(
            "{}/{}@{}",
            REVIEWED_SOURCE.repository, REVIEWED_SOURCE.action_path, REVIEWED_SOURCE.commit
        ),
        _publication: &REVIEWED_SOURCE,
    })
}

#[cfg(test)]
pub(super) fn fixture() -> PublishedFoundationSource {
    published_foundation_source().expect("reviewed published source")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticated_action_source_fixes_node24_and_entrypoint() {
        assert_eq!(
            source_sha256(ACTION_DESCRIPTOR.as_bytes()),
            REVIEWED_SOURCE.action_sha256
        );
        assert!(ACTION_DESCRIPTOR.ends_with(
            "runs:\n  using: node24\n  main: ../owned-cache/foundation-native-entrypoint.mjs\n"
        ));
    }

    #[test]
    fn publication_identity_requires_exact_source_and_api_receipt() {
        validate_tuple(&REVIEWED_SOURCE).expect("reviewed actual publication");
        let mut wrong = REVIEWED_SOURCE.clone();
        wrong.commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(validate_tuple(&wrong).is_err());
        wrong = REVIEWED_SOURCE.clone();
        wrong.tree = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(validate_tuple(&wrong).is_err());
        wrong = REVIEWED_SOURCE.clone();
        wrong.repository_id += 1;
        assert!(validate_tuple(&wrong).is_err());
        wrong = REVIEWED_SOURCE.clone();
        wrong.source_ref = "refs/heads/main";
        assert!(validate_tuple(&wrong).is_err());
        wrong = REVIEWED_SOURCE.clone();
        wrong.action_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(validate_tuple(&wrong).is_err());
        wrong = REVIEWED_SOURCE.clone();
        wrong.independent_review_sha256 =
            "53fe6e702da7e61cd42014813cb6804fd5afd7db1d3765fcb31d49d53bd5c278";
        assert!(validate_tuple(&wrong).is_err());
    }

    #[test]
    fn local_publication_false_receipt_cannot_replace_authenticated_publication() {
        let mut local_only = REVIEWED_SOURCE.clone();
        // Frozen190 joined review explicitly records publication: false.
        local_only.api_receipt_sha256 =
            "81eaa7617a645d608745477df2ab5d6b80f85f8b36c0889d239046df3d9bfe27";
        assert!(validate_tuple(&local_only).is_err());
        local_only.api_receipt_sha256 = "";
        assert!(validate_tuple(&local_only).is_err());
    }
}
