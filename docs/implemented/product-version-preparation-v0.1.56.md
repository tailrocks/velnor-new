# Product version preparation: 0.1.56

The root workspace and exact internal path dependencies advance from 0.1.55 to
0.1.56. The runner workspace keeps its independent version.

The final Required job acquires the published generator and pinned tools before
native report collection and verdict assembly. Candidate checkout remains data;
candidate actions, tools preludes, caches, and tasks stay outside the final
execution boundary. Producer workflows require `.velnor/generator.lock`, which
authenticates SDK 0.1.55 at commit
`693f3aae14406fb7851ac70f0aee647a869b5063` during this preparation.

Open pull-request admission uses the exact merge ref, validates its immutable
commit and ordered base/head parents, and requires an unchanged second ref read.
The public CLI regression exercises this path with real Git history and an
external read-only transport fixture. It does not qualify historical Tap
promotion or a fresh native Tap write.

Historical Tap promotion remains fail-closed. GitHub API version 2026-03-10 no
longer provides `merge_commit_sha` in pull-request responses. A replacement
historical witness, including bounded API retries and binding to source-native
signed release evidence, must be implemented and tested against a genuine
fixture from the first SDK 0.1.56-backed Jackin publication. Structural
same-tip initialization must also be completed and tested against that fixture
before enabling historical promotion.

Before any normal native Tap write, replace the Tap writer's commit DTO fields
that require object types for nested tree and parent responses. Those REST
responses can contain only the SHA values. This issue is independent of SDK
publication: the first signed preview publication can provide the genuine
source fixture while Tap writes remain disabled.

Local compatibility qualification exercised SDK 0.1.56 check outputs with
authenticated published SDK 0.1.55 verdict assembly. Successful checks passed,
an actual failed dependency failed, and missing workspace admission failed with
`cache_corrupt`. These local fixtures do not establish hosted qualification or
authenticate an unpublished SDK 0.1.56 binary.
