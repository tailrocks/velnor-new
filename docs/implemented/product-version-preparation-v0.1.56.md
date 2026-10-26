# Product version preparation for v0.1.56

This source line prepares the Bun runtime correction for the next Velnor Actions SDK release. Consumers must use a published official binary with its verified manifest and provenance before they regenerate workflows.

The workspace version and all 25 exact internal path requirements advance from 0.1.55 to 0.1.56. The correction forces Bun for generated package scripts, Playwright report merging, package creation, and direct pinned Playwright CLI execution. Frozen installation and credential boundaries remain unchanged.

The root lockfile and generated outputs must be refreshed with the 0.1.56 source CLI. Historical release manifests and the separately versioned runner workspace remain unchanged. The existing SDK product release workflow remains the only publication path, including exact-source CI checks, the protected release environment, and immutable verified assets.

This is source preparation. It does not publish a release or create, move, or replace a release tag.
