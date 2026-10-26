# Product version preparation: 0.1.56

The root workspace and exact internal path dependencies advance from 0.1.55 to
0.1.56. The runner workspace keeps its independent version. The CLI public
integration target adds only the existing locked `tempfile` dependency.

The final Required job acquires the published generator and pinned tools before
native report collection and verdict assembly. It retains candidate checkout as
data, while excluding candidate actions, tools preludes, caches, and tasks from
the final execution boundary. Producer workflows require the published
`.velnor/generator.lock`; the lock records authenticated SDK 0.1.55 from commit
`693f3aae14406fb7851ac70f0aee647a869b5063`. Other jobs retain their existing
candidate generator and cache behavior.

GitHub REST API 2026-03-10 no longer supplies `merge_commit_sha` in pull-request
responses. Open pull-request admission reads the exact merge ref, validates its
immutable commit and ordered base/head parents, and requires an unchanged second
ref read. Historical promotion uses GraphQL `mergeCommit.oid`, bound to the REST
repository and pull-request identities. Squash-parent, tree, proof, and lineage
checks remain required. Git commit tree and parent objects use SHA-only DTOs;
Git refs retain their object-type checks.

The public CLI regression uses real Git history and an external read-only
Mise/GitHub transport fixture. A valid REST response without the removed field
reaches the protected-locator gate; reversed parents and a changed merge ref
fail admission. Removing the merge-ref witness produced an observed semantic
failure, and restoring it passed the complete five-test root suite. Generated
CI selects the registered CLI target through package Nextest with
`--no-tests fail`.

Local compatibility qualification also exercised real SDK 0.1.56 check outputs
with authenticated published SDK 0.1.55 verdict assembly. Successful checks
passed, an actual failed dependency failed, and missing workspace admission
failed with `cache_corrupt`. These local fixtures do not establish hosted
qualification or authenticate an unpublished SDK 0.1.56 binary.
