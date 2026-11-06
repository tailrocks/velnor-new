# PR #529 job-selection baseline

Final-head inventory and review dispositions belong in the
[review evidence thread](https://github.com/tailrocks/velnor-new/pull/529#discussion_r4488983545),
bound to the exact reviewed SHA. The snapshots below are historical and cannot
establish current-head compliance.

Observed at initial head `979fa475eae32c4a11f1fecf650853f66f6d7819`.
The [five-file diff](https://github.com/tailrocks/velnor-new/compare/55b31ce8ade5f560bcdd2db7e521f5c0baf41496...979fa475eae32c4a11f1fecf650853f66f6d7819)
changed root `AGENTS.md`, the PR template and three review/reference documents.
This is failed job-selection evidence, not proof that selection is fixed.

## Observed work

| Run | Observation |
|---|---|
| [CI](https://github.com/tailrocks/velnor-new/actions/runs/42159967214) | All 18 Rust jobs ran: 2,005 seconds (33m 25s) summed job execution time, calculated from each job's `started_at` and `completed_at`; other jobs and queue time are excluded. |
| [Maintenance helpers](https://github.com/tailrocks/velnor-new/actions/runs/42159967214/job/127703442632) | Failed on the dogfood golden's PR template and `tree.sha256`. |
| [Source qualification](https://github.com/tailrocks/velnor-new/actions/runs/42159830887) | Three qualification producers started; generation checks found the same template golden mismatch. |
| [Job image candidate](https://github.com/tailrocks/velnor-new/actions/runs/42159770943) | A branch push built and attested an image; publication was skipped. |

## Selection causes

Source inspection at that head explains the broad selection:

- [`decisions.rs`](../../crates/velnor-actions-orchestrator/src/decisions.rs) broadens any `.github/**` change; [`select.rs`](../../crates/velnor-actions-orchestrator/src/select.rs) returns all packages and also broadens unclassified Rust-visible paths.
- [Configured non-inputs](../../.velnor/config.toml) cover only two CI-maintenance documentation trees; the changed instructions/reference files remain unclassified. Mise checks are also added for any Rust-visible or tool-file change.
- [CI](../../.github/workflows/ci.yml) runs Cargo Deny, Cargo Machete, Jactionlint and Ledger CLI without planner selection; each needs its own relevance proof.
- [Source qualification](../../crates/velnor-actions-workflow-renderer/src/schema2_generator_source_qualification.rs) starts producers on every PR; [job image candidates](../../crates/velnor-actions-workflow-renderer/src/schema2_job_image_candidate.rs) build on every push without affected-input selection.

## Real dependency and correction

The template is a real transitive test input: generation preserves it in the
dogfood preview. The maintenance and generation failures therefore correctly
detected changed bytes; an absent declared input does not make the check unrelated.
The follow-up restores the optional template edit to the base version, which is
byte-identical to the [checked-in golden](../proposed/opentofu-goldens/cases/dogfood/preview/.github/PULL_REQUEST_TEMPLATE.md).
Author evidence remains mandatory through root `AGENTS.md` and the review guides.

That correction does not fix unclassified-file broadening or unconditional side
workflows. The remaining generator fix must establish complete input ownership,
route all workload selection through the planner, and prove both relevant execution
and zero unrelated execution under the [review gates](ci-performance.md).
Preserve fail-closed selection and the [hard decisions](../hard-decisions.md);
resolve any required contract amendment explicitly before changing runtime behavior.

## Subsequent audit

At head `d82da760dab88bb36c4407da01f9359a8637bc22`, the
[CI](https://github.com/tailrocks/velnor-new/actions/runs/42161636994),
[source qualification](https://github.com/tailrocks/velnor-new/actions/runs/42161637118)
and [image candidate](https://github.com/tailrocks/velnor-new/actions/runs/42161632042)
workflows passed. Together they executed 33 jobs for 4,979 summed job seconds
(82m 59s), over an overlapping cohort lasting 13m 11s. These are job elapsed times,
not CPU time, billing or proof that all 33 jobs were unnecessary.

The [performance audit tracker](https://github.com/tailrocks/velnor-new/issues/436)
records the remaining selection, reuse, resource and verification findings with
their evidence and acceptance criteria. This PR improves review policy; runtime
findings remain unmet targets. Subsequent changes to the responsible behavior must
fix its relevant inherited violations in the same PR under the improvement rule.
