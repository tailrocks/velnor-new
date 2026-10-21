# Product version preparation for v0.1.36

This prepares the aggregate-to-signer handoff correction for both release
channels. Jackin preview run 39550483154 built all six payloads and assembled
its aggregate successfully. Aggregate admission also succeeded, but signer
job 119068278234 then failed with `package_signer_directory_missing`.
Publication was skipped.

Aggregate admission had staged verified files in `preview-input`, while the
signer required `release-output` on its separate runner. Both operations now
use one fixed signer-owned directory constant. There is no fallback to the
earlier directory. The signer still independently verifies the complete input
before obtaining a signing tool or creating signatures.

The same boundary also flattened companion files to their public asset names
without restoring their configured source paths. Admission now reconstructs
those paths solely from the validated source plan. The original manifest,
policy, source identity, checksums, and closed archive inventory remain intact.
Before creating the signer input tree, destination validation reserves the
handoff manifest, every payload and SBOM, and all companion paths. Exact and
parent/child collisions fail before copying. Parent creation rejects symlinks;
copies retain the existing held-file and exclusive-write checks.

Composed regressions use the actual rendered preview and stable signer
sequence, assembler-format ZIP extraction, production staging, and the real
pre-sign verifier. Both channels include a renamed nested companion. Negative
cases cover path collisions, traversal, symlinks, existing destinations,
inventory substitution, and source-context substitution. Restoring only the
old aggregate directory reproduced the live missing-directory failure at the
actual signer verifier. Restoring the correction passed all 44 focused tests,
strict all-target/all-feature orchestrator Clippy, and formatting.

The signed output, publisher input, attestation subjects, and downstream Tap
handoffs retain their existing paths and security boundaries. Stable release
approval, immutable stable tags, exact-source CI, stale-candidate rejection,
and independently gated Tap writes are unchanged.

The workspace version, 24 exact internal path requirements, and 14 local
package records in the root Cargo.lock advance together to 0.1.36. All 128
registry package records remain identical to the 0.1.35 base. The independently
versioned runner and tool configuration are unchanged.

This records preparation, not a published release. Full root and nested-runner
verification, independent final review, exact-source hosted CI, protected
publication, public artifact verification, and consumer adoption remain
separate requirements. No successful Jackin preview or Homebrew installation
is claimed here.
