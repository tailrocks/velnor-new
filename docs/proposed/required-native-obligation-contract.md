# Required native obligation registry

Status: implemented source contract; runtime regression gates and historical
population remain pending. This registry proves adopted native workload intent.
Rust and OpenTofu configuration, publication, maintenance and other families
remain separate audited requirements. Unsupported entries reject; they never
become zero obligations.

## Input and authority

A repository may supply `.velnor/required-obligations.toml` with schema `1` and
nonempty `[[obligations]]` entries. Components are sorted and unique. Each entry
contains a safe `component`, closed `operation` (`WorkloadKind`), normalized
source `root`, closed `phases` (`RequiredNativePhase`), and `profile_digest`.
Unknown fields, operations, phases, duplicate components, duplicate phases,
invalid roots, unsupported schemas and empty inventories fail validation.

The phase list is a complete unordered inventory. Compiler-owned ordering and
configuration, including package script ordering, bind through the recipe digest.
The registry accepts only phase tokens defined for the selected operation. It
carries no command, argv, shell body or configurable execution expression.

`profile_digest` is BLAKE3 over canonical JSON of the complete validated
`WorkloadConfig`: name, operation, source root, inputs, paths, package scripts,
Gradle/database descriptor, desktop descriptor and updater descriptor when
present. Digests are populated from an independently adopted exact recipe.
Generating a digest from an already reduced current configuration cannot prove
that its historical obligations were retained.

## Admission and execution

`workloads::derive` checks the registry against the complete native proposal
universe before affected-work selection and before its empty-work return. Every
required component must have an exact operation, source root and recipe digest.
Its emitted phases must equal the required inventory, without duplicate phases
or contradictory owners, kind or source roots. A missing declaration, missing
phase or changed recipe fails generation and planning. Additional components may
be adopted independently; their presence cannot satisfy another component.

Registry acceptance authorizes no new execution capability. Pending native
helpers, missing source evidence and invalid profiles still fail their existing
qualification gates before proposal emission. A typed obligation can describe
required intent while its execution owner remains unqualified.

Dedicated source evidence separately guards disappearing declarations:
Docker/package/Swift/Gradle manifests, exact Xcode project source, and explicit
formula/cask source coverage. Formula/cask capability permits exact Ruby syntax
coverage for repositories whose adopted procedure is syntax-only; it does not
invent a Homebrew audit requirement. A reviewed registry requiring strict audit
cannot be satisfied by that weaker syntax declaration.

## Historical population and limits

The 47-repository scope audit remains the population authority. Each row needs
an immutable prior source reference, adopted exact native descriptors, complete
phase mapping, and an explicit disposition for every unsupported or unavailable
family. Private descriptors stay in external evidence. Mapping completion,
execution qualification and rollout qualification are distinct states.

A missing registry does not claim historical completeness. Deleting both the
registry and the only policy evidence cannot be detected from current source
alone. Independent before/after review remains mandatory for migration and
rollout, including requirements without unique source signatures. No green run,
empty workflow, deleted configuration or missing mapping establishes retirement.
