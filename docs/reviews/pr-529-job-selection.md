# PR #529 job-selection baseline

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
