# Qualification fixtures

`testcontainers-rust/` is a standalone, non-published Rust package. Its manifest declares its own workspace, and its checked-in `Cargo.lock` pins the fixture's complete dependency graph. It is not a member of the root Cargo workspace or its package discovery. The repository's `.alint.yml` also treats `fixtures/` as test input, not product source.

The fixture checks local certificate generation, HTTPS hostname verification, and the pinned Testcontainers client contract. No workflow runs it in the paired hosted runner and Docker service namespace. These local checks do not qualify the deployed runner image or prove the paired SDK and Docker path.
