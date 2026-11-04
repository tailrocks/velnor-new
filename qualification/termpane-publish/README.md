# Termpane no-publish rehearsal

This workflow is a provisional rehearsal for termpane 0.1.1. It checks out
`f1c2dfde45bdbe91425af6bb17d80e606703baac`, uses the pinned Linux
release-plz binary, and runs with Docker networking disabled. The fixture routes
GitHub and crates.io requests to a local fake service. Its Cargo interceptor
runs real offline `cargo package` in place of `cargo publish`, then compares
the packaged `src/lib.rs`, `src/grid.rs`, Git commit, and `EraseDisplayPolicy`
with the checkout. It does not compare the complete archive; the published
crate still requires full verification against the approved source.
No real registry or GitHub mutation is performed, and no real registry token or
GitHub OIDC credential is requested.

The per-package config comes from the production renderer, but the typed
intent in the test is a fixture with `pty` enabled. The reviewed termpane
release intent has not yet been generated. After that intent is reviewed,
compare its package, feature selection, version and source with this rehearsal,
and rerun against that exact intent before using the result for the release.
This rehearsal does not verify a real GitHub OIDC exchange, a real crates.io
upload, termpane's protected environment, or the complete generated
publisher and its execution boundary. Those require separate qualification.

The workflow runs only from Velnor's default branch and uses its protected
`generator-release` environment. Supply the current pull request number and its
full head SHA. Before checkout and again immediately before the candidate test,
the workflow verifies the same-repository open PR, current head, clean merge
state, successful checks, the required GitHub Actions check, effective strict
main-branch rules, and resolved review threads. Incomplete API responses fail
closed. Check all reviews, comments, and replies independently, then verify the
same SHA before dispatch and normal environment approval. The workflow does not
establish that all general feedback was addressed. Its read-only permissions do
not configure crates.io or alter credentials. Review the workflow and complete its PR gates
before dispatching it; the required reviewer must approve normally. Do not use
administrator bypass. Environment approval is a separate human action.
