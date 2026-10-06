# Agent runtime audit — 2026-10-04

## Required runtime policy

The updated task instruction sets implementation, research, and test work to
`gpt-6-luna` at `max`. Independent review may use `gpt-6.1-sol` at `medium`.
`gpt-5.6` and `gpt-6-astra` do not satisfy this task policy.

## Incident

Some nested tasks were spawned with full-history forks after the policy changed.
They inherited `gpt-5.6-luna/max`; the prompt labels did not change their runtime.
The affected work was read-only review or consumer research:

- lineage security review;
- full Git-ref grammar audit;
- generated-output preservation architecture and final reviews;
- OpenTofu candidate-diff and caller reviews;
- three early consumer-audit cohorts.

Their reports remain useful investigation leads. They do not count as independent
acceptance evidence. No product-source changes came from these nested tasks.

## Remediation and status

- Active noncompliant review tasks were stopped. Their notes and process evidence
  were preserved; no worktree or user data was discarded.
- Direct implementation lanes were checked against their own latest
  `turn_context`; the active lanes use `gpt-6-luna/max`.
- Consumer cohorts A, B, and C were reissued as `gpt-6-luna/max`; their fresh
  reports, plus the remaining deep-cohort reports, own the current scope audit.
- Required source reviews are being rerun by independent
  `gpt-6.1-sol/medium` reviewers after the exact source commit is frozen.
- The separately completed Jackin deep review used the permitted
  `gpt-6.1-sol/medium` review runtime and remains valid review evidence.

## Verification rule

For every accepted delegated result, bind the exact spawned task to its
`session_meta` agent path and parent thread, then verify the latest `turn_context`
model and effort. A prompt label, inherited parent metadata, or an unrelated
repository JSONL is not runtime proof. Spawn descendants with `fork_turns: none`
and an explicit model and effort.

This is a status record for the 2026-10-04 task. Recheck remaining reviews and
consumer cohorts before final acceptance.

## Phase-C checkpoint and coordination incidents

At integration HEAD `97abf91ff210a6fe29e275b6dd6c83f8e1cb8b14`, the pushed
changes are documentation-only. The exact source/test units below are detached
candidates, not part of that HEAD; their scoped gates do not establish hosted
qualification.

| Candidate | Verified checkpoint | Status |
| --- | --- | --- |
| Resource and corruption probes | Source `fd342e63` received scoped source-review GO. Safety fixture `3dcd23a`: native Ubuntu 24.04.4 ARM64, 4/4 in 95.89 s; independent Sol/medium fixture review GO. Earlier receipts are separate runs. | HOLD: stock `actions/cache` clean MISS leaves hit/matched outputs unset; `resource_stop.sh` requires `cache_hit=false` and rejects clean MISS. The writer identity guard can treat unavailable/swallowed restore errors as cold. Fix and re-review before composition. No hosted x64 result. |
| Parallel cache probe | `a094076d`; 17 source tests and exact Sol/medium review. | HOLD after stock `actions/cache` audit: clean MISS leaves hit/matched outputs unset, while the probe expects `cache_hit=false`; update and re-review before integration. Hosted run unexecuted. |
| Cancellation probe | `850e9aea`; 22 renderer unit fixtures, registered renderer/orchestrator tests 1/1 each, strict renderer Clippy on Rust 1.98.1/Clippy 0.1.98. Exact security and correctness reviews returned GO. | Source-scope GO only. Strict cross-host pre/post timing is not established by unbounded clock comparison; hosted run unexecuted. |
| Official Mise | Public refresh `2026-10-04T12:10:38Z`: stable `2026.10.1`, release `402539535`, tag `b752bdc` peels to `050ce5a`; accepted fix `dfe74a90` remains eight commits ahead. | Adoption remains `PARTIAL`; no fixed official distribution. |

On `2026-10-04T11:18:32.580Z`, parent
`/root/velnor_coordinator_v2/mbx_registration_workflow_v17` spawned
`/root/velnor_coordinator_v2/mbx_registration_workflow_v17/renderer_test_pattern_scout`
without explicit model or effort (parent log
`rollout-2026-10-04T18-17-36-01a106a2-35ac-78f1-943d-b08f8b0bd9b2.jsonl`,
ordinal 41; `fork_turns=none`). Its child task was recorded at ordinal 9 in
`rollout-2026-10-04T18-18-32-01a106a3-119f-7930-a2d2-fee87e5ebf39.jsonl`;
session `01a106a3-119f-7930-a2d2-fee87e5ebf39` turn
`01a106a3-126a-7c52-ad03-cab727c73565` used `gpt-5.6-luna/max`. The scout
report was rejected as acceptance evidence; no product change came from it.

The documentation actor `/root/cache_progress_evidence_docs_v20` committed
`fedf114a770f4e8a1d77cde7fedd87ec4b819fb4` on the shared integration worktree
instead of its assigned isolated worktree.
`git show` confirms one file only, `docs/implemented/cache-measurements.md`
(25 added lines); it was not pushed at that point. The coordinator later
reviewed and corrected the two imprecise claims in DCO signed-off commit
`c21f10d0c3ba5eee99580335ca5024a5c38353a6`, which was pushed. The two
pre-existing dirty config/test paths remained outside both commits.

## Follow-up audit and remediation

A later actor-by-actor audit bound each child through parent spawn, returned
handle, child task record, and latest `turn_context`. It found 13 additional
nested tasks using `gpt-5.6-luna/max`, plus two earlier OpenTofu reviewers that
resumed at that model. No `gpt-6-astra` actor was found.

The 13 newly found nested actors were:

| Actor below its direct parent | Session ID | Latest turn UTC | State at audit |
| --- | --- | --- | --- |
| `branch_safe_grammar_impl_v2/branch_caller_audit` | `01a10469-e4e2-7c01-8ac7-e235e8894522` | 00:56:53.632 | completed |
| `branch_safe_grammar_impl_v2/pr12_grammar_evidence` | `01a10469-f74e-7061-815c-33855e8eaa92` | 00:56:58.480 | completed |
| `phase_d_closure_audit_v2/phase_d_python_builder_audit` | `01a10477-9068-7c03-9316-f24897800755` | 01:11:51.059 | completed |
| `phase_d_closure_audit_v2/phase_d_named_scripts_audit` | `01a10477-a66b-7e10-9aec-a7e166c5a30f` | 01:11:55.154 | completed |
| `phase_d_closure_audit_v2/foundation_closure_owners` | `01a10477-b76f-7750-9c0c-ff2279ee5ff9` | 01:12:00.107 | completed |
| `typed_suite_tool_owner_impl_v2/typed_owner_review` | `01a1047a-5a5a-76f3-82e2-2e6826f37663` | 01:14:53.589 | interrupted |
| `branch_ref_grammar_impl_v2/branch_ref_exact_review_v2` | `01a1047b-037f-7241-be1a-01fcc486efac` | 01:15:35.675 | interrupted |
| `branch_ref_grammar_impl_v2/branch_ref_exact_review_v3` | `01a1047d-eaac-7fd3-9522-968571e079fc` | 01:18:47.206 | completed |
| `foundation_detachment_prep_v2/foundation_workflows_cli` | `01a10483-c223-7940-b1e8-3e699dd4fdb8` | 01:25:09.673 | interrupted |
| `foundation_detachment_prep_v2/foundation_orchestrator_render` | `01a10483-d8dc-7130-914b-30a21a388b61` | 01:25:17.099 | interrupted |
| `foundation_detachment_prep_v2/foundation_docs_release` | `01a10483-ede4-7e43-801f-a86a886865ae` | 01:25:21.974 | interrupted |
| `pr12_disposition_ledger_v2/pr12_evidence` | `01a10486-1ae5-7ee0-b3c1-1a8985bc0321` | 01:27:44.778 | interrupted |
| `pr12_disposition_ledger_v2/sha_mapping` | `01a10486-2d69-7cb0-88d3-ac5f001312e9` | 01:27:47.616 | interrupted |

The two earlier OpenTofu reviewers also resumed at 5.6: `candidate_diff` at
01:17:39.800 UTC and `current_tofu_review` at 01:20:17.759 UTC. Their reports
remain provisional.

The three active Foundation preparation tasks and two active PR12 evidence
tasks were interrupted after preserving their files and notes. The other
noncompliant nested reports are provisional leads. They are not independent
acceptance evidence. No implementation patch was produced by those nested
actors; compliant direct-parent source work and processes were preserved.

The following replacement actors were independently checked against their
spawn records and exact session logs, all with `fork_turns: none`:

- branch shorthand current-integration review: `gpt-6.1-sol/medium`,
  log `rollout-2026-10-04T08-37-56-01a1048f-81e4-7443-b140-83c9cced20bc.jsonl`,
  session `01a1048f-81e4-7443-b140-83c9cced20bc`, turn
  `2026-10-04T01:37:58.777Z`;
- typed suite/tool-owner review: `gpt-6.1-sol/medium`, session
  log `rollout-2026-10-04T08-38-01-01a1048f-965c-7441-81d7-0a18340a546a.jsonl`,
  session `01a1048f-965c-7441-81d7-0a18340a546a`, turn
  `2026-10-04T01:38:03.847Z`;
- Foundation/PR12 closure preparation: `gpt-6-luna/max`, session
  log `rollout-2026-10-04T08-38-05-01a1048f-a73d-7cd0-92ee-9f357e065dad.jsonl`,
  session `01a1048f-a73d-7cd0-92ee-9f357e065dad`, turn
  `2026-10-04T01:38:08.310Z`.

The new reviews do not retroactively qualify earlier nested reports. Direct
owners must recheck any implementation or disposition that relied on those
reports. The full-ref grammar slice was separately revalidated on its frozen
integrated blob by an allowed reviewer; the branch shorthand current-head
candidate and typed owner change each have separate source/test gates.

## Follow-up resumption incident

At 2026-10-04 02:49:45 UTC, a follow-up call resumed the completed
`/root/upstream_mise_release` actor from its pre-policy session instead of
starting a fresh actor. The follow-up therefore retained the old runtime:

| Evidence | Value |
| --- | --- |
| Existing actor session | `01a10437-6944-7c62-abdb-ec68e085d6f8` |
| Existing actor log | `rollout-2026-10-04T07-01-42-01a10437-6944-7c62-abdb-ec68e085d6f8.jsonl` |
| Follow-up call | Coordinator log `rollout-2026-10-04T07-13-35-01a10442-4a92-7211-817b-adfe1e03ae60.jsonl`, line 7182, `call_KJHdztKOjDnMJl72QK8qS4a4`, `2026-10-04T02:49:45.542Z` |
| Latest resumed turn | `01a104d1-434f-77e0-87a6-6360efb50564`, `2026-10-04T02:49:45.692Z` |
| Actual runtime | `gpt-5.6-luna/max` |
| Child `NEW_TASK` | Log line 528, coordinator sender |

That report is provisional and does not establish current official-release
status. The old actor was not resumed again. A fresh replacement was explicitly
spawned with `model=gpt-6-luna`, `reasoning_effort=max`, and `fork_turns=none`:

| Evidence | Value |
| --- | --- |
| Replacement actor | `/root/official_mise_refresh_v3` |
| Spawn | Root log `rollout-2026-10-04T06-50-27-01a1042d-1d81-7771-9165-c462f3fb2b4d.jsonl`, line 3781; returned handle line 3784 |
| Child session/log | `01a104d4-f9a5-75f3-bd3f-7f0252b64f05`; `rollout-2026-10-04T09-53-48-01a104d4-f9a5-75f3-bd3f-7f0252b64f05.jsonl` |
| Parent thread | `01a1042d-1d81-7771-9165-c462f3fb2b4d` |
| Verified turn | `01a104d4-fc54-78e0-bbcb-9afa2b2164b0`, `2026-10-04T02:53:54.784Z` |
| Actual runtime | `gpt-6-luna/max` |
| Child `NEW_TASK` | Log line 10, `2026-10-04T02:53:54.790Z` |

The replacement found no published fixed mise release; official adoption remains
PARTIAL. Its runtime was independently bound through the root spawn record,
returned handle, child task record, and child turn context. This incident adds
no product-code change and does not qualify the earlier actor's report.

## Follow-up reviewer model-policy incident

At `2026-10-04T05:45:58.925Z`, the compliant implementation actor
`/root/version_fixture_closure_v4` spawned a nested independent reviewer
without a model and with a full-history fork. The parent actor itself was
verified `gpt-6-luna/max`; that did not override the nested call's inherited
runtime.

| Evidence | Value |
| --- | --- |
| Parent session/log | `01a1056e-8937-7b10-94ee-28b58d9724e0`; `rollout-2026-10-04T12-41-32-01a1056e-8937-7b10-94ee-28b58d9724e0.jsonl` |
| Parent runtime | Latest turn `01a1056e-8a07-7d22-a049-1504a5e0c878`, `2026-10-04T05:53:54.754Z`, `gpt-6-luna/max` |
| Spawn | Parent log line 152, call `call_OhFB3Kv9LvOrll0o5XIv8kPw`, `2026-10-04T05:45:58.925Z`; `model` omitted, `reasoning_effort=medium`, `fork_turns=all` |
| Returned handle | Parent log line 155: `/root/version_fixture_closure_v4/versionfixture_independent_review` |
| Child task | Child log line 16, `2026-10-04T05:46:02.317Z`, sender `/root/version_fixture_closure_v4` |
| Child log | `rollout-2026-10-04T12-45-59-01a10572-99d4-7e93-b44d-4b55a3bb1eab.jsonl`; child thread `01a10572-99d4-7e93-b44d-4b55a3bb1eab`; parent thread `01a1056e-8937-7b10-94ee-28b58d9724e0` |
| Invalid review turn | Child log line 14, turn `01a10572-9af4-7462-acc7-478435e4122f`, `2026-10-04T05:46:02.254Z`, actual `gpt-5.6-luna/medium` |

The nested report is excluded as independent acceptance evidence. The old
reviewer was interrupted after the mismatch was found; no code changes came
from that actor. The implementation candidate itself remains valid work from
the parent `gpt-6-luna/max` actor.

### Fresh exact review

The owner then obtained a new independent exact review with explicit
`gpt-6.1-sol/medium` and `fork_turns=none`:

| Evidence | Value |
| --- | --- |
| Spawn | Parent log line 825, call `call_a7LFQSVFXUIxveQ3TilLClcY`, `2026-10-04T06:10:48.814Z`, model `gpt-6.1-sol`, effort `medium`, fork `none` |
| Returned handle | Parent log line 828: `/root/version_fixture_closure_v4/versionfixture_final_review` |
| Reviewer log/session | `rollout-2026-10-04T13-10-48-01a10589-557c-7da1-9df5-4146dc3a969c.jsonl`; child thread `01a10589-557c-7da1-9df5-4146dc3a969c`; parent thread `01a1056e-8937-7b10-94ee-28b58d9724e0` |
| Verified review turn | `01a10589-57df-7ef0-8f3a-d3947dd4d869`, `2026-10-04T06:10:52.449Z`, actual `gpt-6.1-sol/medium`; a later reviewer turn at `06:15:24.030Z` retained the same model/effort |
| Reviewed source | `fbff07adba5f6e33b4dc8d41ed1f39216908488b` |
| Result | `GO`; no current-default release-identity counterexamples. The duplicate current `0.1.0` manifest fixture was removed by deriving the expected manifest from the existing package-version fixture authority. |

The fresh review replaces the earlier provisional triage; it does not retroactively
make the old reviewer compliant. No product-runtime claim or PR merge follows
from this documentation correction.

## Follow-up descendant spawn incident

On 2026-10-04 at 06:37 UTC, a compliant release-workflow implementation actor
spawned two grandchildren without explicit model/effort overrides and requested
full-history forks. The grandchildren inherited the older model default. The
parent actor itself was `gpt-6-luna/max`; parent compliance did not propagate to
new child spawns.

| Evidence | Value |
| --- | --- |
| Parent session/log | `01a105a0-e4c4-7f61-ba91-ea83879d678c`; `rollout-2026-10-04T13-36-32-01a105a0-e4c4-7f61-ba91-ea83879d678c.jsonl` |
| Parent latest turn | `01a105a0-e7b1-7ca0-807b-79dea9bd61f4`, `2026-10-04T06:57:52.504Z`, `gpt-6-luna/max` |
| First spawn | Parent log zero-based record index 36 (line 37), `2026-10-04T06:37:12.959Z`; call `fc_0837f7d3faf136bb016ac1f416c7e087d08e5f756ec54c2dad`; model/effort omitted, `fork_turns=all` |
| First returned handle | Parent log zero-based record index 39 (line 40): `/root/velnor_coordinator_v2/release_exact_artifact_qualification_fix_v7/release_qual_research` |
| First invalid turn | `01a105a1-8313-7253-917e-4650b4191b74`, `2026-10-04T06:37:16.049Z`, actual `gpt-5.6-luna/max`; child log `rollout-2026-10-04T13-37-13-01a105a1-81a1-7683-97b6-bd0474c2f07e.jsonl` |
| Second spawn | Parent log zero-based record index 42 (line 43), `2026-10-04T06:37:16.626Z`; call `fc_0837f7d3faf136bb016ac1f41ac10c87d0a9d3233ef2fb5f59`; model/effort omitted, `fork_turns=all` |
| Second returned handle | Parent log zero-based record index 45 (line 46): `/root/velnor_coordinator_v2/release_exact_artifact_qualification_fix_v7/bootstrap_contract` |
| Second invalid turn | `01a105a1-904a-7003-8234-bb44e455576e`, `2026-10-04T06:37:19.134Z`, actual `gpt-5.6-luna/max`; child log `rollout-2026-10-04T13-37-16-01a105a1-8fde-7de3-8b2c-0df03ed99344.jsonl` |

Both invalid actors were interrupted, and their reports are not accepted as
review evidence. The same implementation parent started explicit
`gpt-6-luna/max`, `fork_turns=none` replacement research actors:
`release_qual_research_v2` verified at turn
`01a105a1-dee8-70e1-b5cd-88724faf774f` (`06:37:39.400Z`) and
`bootstrap_contract_v2` at `01a105a2-119d-7802-8296-aee8198906a5`
(`06:37:53.112Z`). The first replacement's narrow metadata recommendation was
rejected; use the second report only as source-contract research, not as
independent review. The workflow source still requires exact-candidate behavior
qualification and a separate allowed-model review.

The enabling condition was decentralized worker spawning with inherited
defaults. `AGENTS.md` now assigns all spawns to root or the task coordinator;
workers request bounded follow-up work instead. New spawns must set model,
effort, and `fork_turns=none`, then verify the exact handle's latest turn
context before relying on output. This audit does not treat coordinator message
delivery as independently verified evidence.
