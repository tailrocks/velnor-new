# Generator native population evidence

Generator scope row 0 has a separately reviewed native source map. Native
population is closed for the inspected history: **zero adopted native recipes**,
not zero CI obligations. Execution, rollout and performance remain unqualified.

## Source and intent

Five immutable complete tracked trees and workflow/config/task inventories cover
PR #1 merge `8b268d17…`, PR #5 merge `c6eee580…`, PR #10 merge `95c1d6f0…`, frozen
W0 `c57c700459bbe1549fe7eedcb7d8689585c38986`, and committed PR #12 head
`6209c06d87c2f7e41ff162b77cc51e0c99989eec`. No Docker, Node/Bun package, Swift/Xcode,
Gradle or Homebrew manifest/declaration, or required-native registry, appears.
Rust/configuration and complete workflow/task declarations establish the native
absence; the formerly missing source-index row does not establish it.

Both W0 and that PR head retain eight Rust lanes, **47 Rust phases and 11
external control/maintenance entries**. Full commands, environments, events,
conditions, dependencies and pinned action invocations are retained. Seven
library lanes have format, clippy, build, test, doctest and documentation; the
CLI lane has the same inventory without doctest. Policy validators, candidate
helper build, generated-tree verification, Plan, Required, trusted baseline
publication and freshness remain external requirements. Synthetic Tofu fixtures
are excluded from native discovery and remain inside Rust integration tests.
No recipe digest or native registry was created.

Actual PR checkout is integration commit
`9927a4907c1b3aeb4cc2043df823c62cdcd80935`, proven by Plan log lines 145/148.
Its immutable tree equals the audited PR head tree
`c2d3f61d85b742094a4515f65c9d14d7505b4420`. The feature head and tested
candidate identities remain separate.

## Actual outcomes and later source

W0 run `37012391691`, attempt 1, has 15 executed jobs and 15 raw logs:
orchestrator Format and Required fail; baseline publication is unexecuted.
PR run `37075875245`, attempt 1, likewise has 15 executed jobs and 15 raw logs:
orchestrator Clippy, CLI tests and Required fail; baseline is unexecuted.
No executed log is missing. API phase outcomes are observations, not validated
semantic coverage. Freshness's fully paginated run inventory contains zero
runs; its schedule/manual source is audited, with no runtime result inferred.

PR #12 later advanced to `92fee7465294af8f721319ccaec6d1308832d9df`.
A separate untruncated remote tree and seven exact blob-verified source files
retain that observation. It adds `velnor-actions-native` as a ninth **Rust**
workspace member while generated CI retains eight Rust lanes. Required Rust
coverage needs reconciliation. This source observation does not create an
adopted native recipe or inherit the earlier PR run's validation.

## Immutable evidence and correction

Durable private evidence lives under
`~/.codex-chainargos2/private/ci-performance/generator-source-map/`.
Corrected immutable map:
`source-map-24d2d53886ed5fa9497d16992915f26015f95b1ae74aae42a4b676e83c1d54d1.json`.
Independent review: `independent-review-24d2d538.md`.

The previous reviewed map `967dccf2…` is preserved byte-for-byte at its SHA-named
path. Its floating PR metadata/diff had advanced to `92fee746…` and could not
bind the earlier `6209c06…` attempt. The corrected map explicitly separates
that metadata and binds a new immutable W0-to-6209 diff, SHA-256
`b5ec3c41a89a2cc8ad4397d7023b839b2c634f6fca9f15c6d46c8300c8788509`.
Original source references, trees and raw executed logs are unchanged.
The focused independent review verified the correction and external Rust gap.

This source map closes native population only. The separate
[runtime audit](ci-performance-runtime-audit.md) now closes the finite generator
W0 source/log/artifact audit with explicit unavailable observations and
independent full-workflow/semantic reviews. Execution, rollout and controlled
performance qualification remain incomplete. Frozen archive bytes remain
untouched; current dirty worktree changes are excluded.
