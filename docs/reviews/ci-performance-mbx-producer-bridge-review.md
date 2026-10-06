# MBX producer bridge review

Status: **static draft review only; operational and hosted qualification incomplete**. Parent reviewer and independent `mbx_bridge_review` reviewer inspected actual current source on 2026-10-03 using the requested model configuration. No Cargo, Git or cache operations performed.

## Accepted structural boundary

`orchestrator/src/mbx_producer.rs::DraftMbxProducer` has private source/job/catalog/setup/version fields. `validate()` reconstructs the complete compiled source factory, compares every helper record and regenerates the complete writer job. `validate_recipe()` additionally compares the structural recipe's original job, exact ordered source helpers and generator marker. This binds computation to its closed source owner; it is stronger than operation labels or a matching payload hash.

`renderer/src/cache_mbx_roles.rs::validate_mbx_producer` validates only the structural draft: exact role/selection/cohort/scheduling, six step bindings, registered helper invocation/environment, cache transport, output bindings and permissions. Its result does not establish artifact provenance, signing authority or valid warm state. This separation is appropriate provided signing source creation remains private and requires the independently reconstructed `DraftMbxProducer` capsule.

## Current qualification limit

`orchestrator/src/mbx_admission_source.rs` currently has only `MbxCapturedSourceInputs::UnsupportedPublication`. All four emitted helpers explicitly report unavailable preparation/admission/verification/publication. No bytes are downloaded, executed, admitted as MBX state or promoted. The actual artifact-origin/execution-witness attribution implementation therefore does not exist in this inspected owner path yet.

The private `draft_cache_receipt_sources_with_mbx` bridge was not present at the inspected snapshot. Receipt admission currently reconstructs tool/native owners and rejects other operations. `CacheProducerRecipe` currently admits only tool/native roles and rejects nonzero `actions` permissions. This honestly blocks MBX signing integration today; it is not a completed MBX cache implementation.

Fixed artifact names, source SHA, successful producer-job conclusions and GitHub upload metadata alone must not become execution attribution. Repository tasks may upload a same-name artifact with runtime credentials. The admitted data owner must establish the supported native bundle, comparison-state/delta, exact tool/ABI/cohort, selected successful producer work, every retained historical origin and source-qualified publication without attributing unrelated or bypassed compilation to a witness. No proposed static bridge supplies that evidence.

## Integration requirements and actionable issue

1. Keep the bridge private. Before any receipt source is generated, invoke `DraftMbxProducer::validate_recipe`, regenerate the complete source factory/job, and compare full records/environment/owner identity. Do not introduce public renderer owner-proof constructors or a generic draft-to-signing authority path.
2. Preserve MBX-only `actions: read` for same-run artifact service queries in the reusable producer. Current `cache_producer_workflow.rs::workflow_yaml` emits only ID-token/attestations permissions; blindly reusing it would lose required artifact-read authority. Do not broaden other pure producer permissions.
3. Clearing intra-workflow needs when constructing a reusable callee is acceptable only with caller-side exact selected-work/prerequisite admission and the callee's independently authenticated data owner. Preserve those bindings explicitly; trusted-push alone is not producer attribution.
4. Verify changed owner source, invocation, environment, extra job step, scheduling/cohort, permission, artifact name/origin, comparison state and producer failure negatives. Source/API tests are separate from actual hosted publication and warm measurements.
5. **Fixture defect found:** `mbx_admission_source_tests.rs` uses producer job `validate-rust`, while validation-domain descriptors now require a `rust-` job ID. Its `sources()` calls `compiled_mbx_sources` → `metadata.validate`, so three tests fail before their intended assertions. Reported to the pure-writer owner; corrected fixture and queued Cargo proof are required.

No warm-authority bypass was found in the inspected cold-only source owner. No approval is given for future admission logic, publication, artifact witnesses, persistence, real compiler work or T01–T26 hosted behavior. Re-review the landed bridge and operational owner implementation at their final source identity.
