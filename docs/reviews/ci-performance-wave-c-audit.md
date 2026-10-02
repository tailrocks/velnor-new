# Wave C performance audit

Read-only W0 audit, 3 October 2026. Scope is the five ChainArgos consumers in
the exact 47-repository inventory. No dispatch, push, merge, protection change,
release, or deployment occurred. The latest explicit execution instruction
selects `gpt-6.1-sol` with medium reasoning for delegated work.

## Qualification boundary

Full current workflow families, typed configuration, runtime manifests, relevant
migration diffs, recent workflow changes, default-branch identity, and hosted
PR/default-run evidence were assigned separately per repository. Authenticated
source, diffs, artifacts, job metadata and raw logs live outside this public
checkout in `/tmp/velnor-ci-performance-wave-c/`. Each repository directory
contains an evidence index and audit summary; the initial paginated PR metadata
is shared from `/tmp/velnor-ci-performance-scope-audit/`.

| Repository | Visibility | Audited default SHA | Migration PR | Qualification |
|---|---|---|---|---|
| `ChainArgos/blockchain-nodes` | public | `0881638712a837bdcb90b3cd811a8a9be3aa8d83` | [729](https://github.com/ChainArgos/blockchain-nodes/pull/729) | INCOMPLETE |
| `ChainArgos/java-monorepo` | private | `5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45` | [2081](https://github.com/ChainArgos/java-monorepo/pull/2081) | INCOMPLETE |
| `ChainArgos/jackin-agent-brown` | private | `6b2ac2277103a22b9ae155bc92415ca5ddf05f95` | [243](https://github.com/ChainArgos/jackin-agent-brown/pull/243) | INCOMPLETE |
| `ChainArgos/cloudflare-tofu` | private | `cc0e2b687dc6c1a9943b0ae9f5911f1e782c1a20` | [6](https://github.com/ChainArgos/cloudflare-tofu/pull/6) | INCOMPLETE |
| `ChainArgos/github-terraform` | private | `c8d47ea87a611251f965555dafbd8015f29f303b` | [14](https://github.com/ChainArgos/github-terraform/pull/14) | INCOMPLETE |

All five remain `INCOMPLETE`: this audit precedes qualified regeneration and
rollout. No controlled isolated cold/warm/third-run experiment exists for these
audited revisions. Historical successes, cache inventory, and immutable-key
hits do not establish performance qualification. Unknown compiler, linker,
queue and transfer telemetry remains unknown.

Representative migration-run references below are provenance only. Full raw
logs were inspected; success is not a performance or obligation-equivalence
claim. Attempts are 1 unless the external index records otherwise.

| Repository | PR run | Default run | Observed execution |
|---|---|---|---|
| `ChainArgos/blockchain-nodes` | [37027342900](https://github.com/ChainArgos/blockchain-nodes/actions/runs/37027342900) | [37028375599](https://github.com/ChainArgos/blockchain-nodes/actions/runs/37028375599) | success; prior coverage differs |
| `ChainArgos/java-monorepo` | [37026992841](https://github.com/ChainArgos/java-monorepo/actions/runs/37026992841) | [37027060262](https://github.com/ChainArgos/java-monorepo/actions/runs/37027060262) | failed validation; baseline unavailable |
| `ChainArgos/jackin-agent-brown` | [37027272296](https://github.com/ChainArgos/jackin-agent-brown/actions/runs/37027272296) | [37027341624](https://github.com/ChainArgos/jackin-agent-brown/actions/runs/37027341624) | success with no selected substantive work |
| `ChainArgos/cloudflare-tofu` | [37026608163](https://github.com/ChainArgos/cloudflare-tofu/actions/runs/37026608163) | [37026620591](https://github.com/ChainArgos/cloudflare-tofu/actions/runs/37026620591) | substantive success; performance unqualified |
| `ChainArgos/github-terraform` | [37026792292](https://github.com/ChainArgos/github-terraform/actions/runs/37026792292) | [37026808351](https://github.com/ChainArgos/github-terraform/actions/runs/37026808351) | substantive success; performance unqualified |

Historical repetitive release output has been parsed and compared across every
job, step, command, action, option and security attribute; unique patterns were
read and exact contracts retained. This also revealed an existing publication
gate that does not enforce every validation dependency. Restoration must repair
that fail-open behavior. Historical empty log archives, HTTP 404 responses and pending
executions are separately indexed; available current representative logs do not
recover unavailable historical step evidence. External summaries retain exact
window dates, pagination counts, attempts, jobs and availability reasons.
All five repositories have complete bounded recent-PR path screens, including
tool/configuration changes under generic titles. Relevant API-omitted bodies
were recovered from immutable full files and compared. Full relevant patches
and unique repeated commands were read, with action, environment, permission
and condition differences compared. This closes the identified W0 static audit
gaps; unavailable historical step evidence remains explicitly qualified.

## Findings requiring upstream qualification

Independent source review confirms that whole-workflow replacement can discard
validation, publishing, security and maintenance obligations without a complete
repository-wide disposition. Current generated success therefore cannot prove
equivalence to the previous workflow family. Private findings and exact workload
contracts remain in external evidence for upstream implementation and review.

For public `blockchain-nodes`, PR #729 removes the previous Docker validation
and tag publishing families while Docker inputs remain in the repository.
The generated current run succeeds on a much narrower workload. The old DCO
workflow was removed, but an external DCO application supplies a successful
status on the migration head; workflow deletion alone does not prove a missing
required DCO check. Before treating the migration as complete, record explicit
retirement or qualified replacement for every original obligation.

Audited installed runtimes exhibit tool/source ownership, cache identity,
snapshot-writer and late-selection defects covered by C01–C07. These are
runtime-bound observations. Current generator source has already changed in
some areas, including guarded Plan Rust setup; qualification must regenerate
with the exact new source-bound artifact before asserting a remaining product
defect. Real PR logs also require integration-candidate versus recorded plan
identity review before affected-work proof can be accepted.

## Private authority and hosted evidence

Existing external migration records identify a narrow administrative merge
waiver for the four live-confirmed private repositories. The public repository
is excluded. Independent authority review confirms that the waiver retains
static/security review and exact reviewed-head merge requirements; it grants no
protection changes, forced benchmark dispatch, or performance proof.

Private hosted logs are accessible for several audited PR/default revisions.
Some executions fail; other successes provide incomplete obligation coverage.
Privacy is therefore not a blanket reason to waive available evidence. A future
`CI_WAIVED_PERF_UNVERIFIED` outcome requires an actually applied, qualified waiver
and must remain distinct from green CI and `PERF_VERIFIED`.

## Remaining acceptance work

1. Preserve or explicitly dispose every prior validation/CD obligation through
   typed generator support; independently compare conservative full coverage.
2. Regenerate with the qualified runtime, review exact PR head and resulting
   default branch in the required Wave A → B → C order.
3. Execute applicable T01–T26 with raw fresh-runner evidence, including useful
   late-produced snapshot persistence and exact candidate/proof identities.
4. Record unavailable execution or measurement with the exact reason. No waiver
   or old successful run closes a performance gate.
