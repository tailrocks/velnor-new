# Mise `.miserc` Phase 0 evidence

Status: verified with official Linux x86_64 release binaries. macOS qualification is pending; this evidence does not claim all-platform completion.

Upstream [jdx/mise#13926](https://github.com/jdx/mise/pull/13926) added the `--no-config` and `MISE_NO_CONFIG=1` guards to `.miserc.toml` discovery. Its merge commit is `dfe74a90b41603625ee6aabecb42f14a1f5eb0f6`. The release/tag sources and official release asset metadata are:

| Release | Published (UTC) | Peeled tag source | Official Linux x64 executable asset SHA-256 | Official Linux x64 tarball SHA-256 |
| --- | --- | --- | --- | --- |
| [v2026.10.1](https://github.com/jdx/mise/releases/tag/v2026.10.1), affected | 2026-10-03 14:12:48 | `050ce5a20287a0aafd872b1191699a5fdafff5ac` | `31e6859cf639ed4594906da3fcd0fe2055e9daddae75e9786dbe50b3fb3c0f4a` | `9b92aa39b8fde54b28c8f974a68f2501925a1523d6c05a52719145df3acdd75a` |
| [v2026.10.2](https://github.com/jdx/mise/releases/tag/v2026.10.2), fixed | 2026-10-04 12:31:22 | `44ea2537166efbe21b19d808355d9914e830941a` | `8f5f6660336f572830e33cd9b378d3131e529a0d4c4f0c553776be90a1ba302a` | `79a2bf0ffc9b8a9a6391344e875b3c3679c15053fda3e8728ddf1790d63db788` |

The harness verified each executable as an absolute regular file, hashed the bytes, and ran `mise version` before behavioral assertions. The observed executable hashes matched the official standalone asset SHA-256 values above. The fixed v2026.10.2 executable was extracted from the official tarball after checking its official asset digest.

The registered `velnor_mise` integration harness keeps flag-only and environment-only selection as distinct invocations. It exercises `version` and `exec` at the project root and a nested directory with malformed project, global, and system config layers; checks affected v2026.10.1 controls; confirms malformed config is reached when no selector is used; and exercises the production `IsolatedCommand` boundary against the fixed real binary. The required binary variables are `VELNOR_MISE_AFFECTED_BINARY` and `VELNOR_MISE_FIXED_BINARY`; a missing path, wrong digest, or wrong release is an error in the selected tests.

On 2026-10-05, this exact focused Cargo invocation selected the four explicitly ignored official-binary tests:

```sh
CARGO_TARGET_DIR=/tmp/velnor-phase0-target \
VELNOR_MISE_FIXED_BINARY=/tmp/velnor-mise-fixed-2026.10.2/bin/mise \
VELNOR_MISE_AFFECTED_BINARY=/root/.local/bin/mise \
/root/.local/bin/mise --no-config --no-env --no-hooks exec rust@1.98.1 -- \
cargo test --locked --jobs 2 -p velnor-actions-mise --test velnor_mise \
impl_miserc_isolation:: -- --ignored --test-threads=1 --nocapture
```

Result: **4 passed, 0 failed, 0 ignored, 341 filtered out**. The raw output is retained at `/tmp/velnor-phase0-miserc-tests.log` for this local run. The local Rust tool installation has no `cargo-nextest` command, so no local Nextest execution is claimed. The CI route uses Nextest's `--run-ignored only` spelling and must hard-fail if its pinned binary setup is absent. macOS and the CI-selected run remain pending evidence.

The same existing integration binary also contains the generic optional-lock regression port. It verifies that the production `GitRequest`/`IsolatedCommand` overrides hostile parent `GIT_OPTIONAL_LOCKS=1`, that status does not refresh the index with the effective value `0`, that the direct `1` control does refresh it, and that explicit add, commit, and clone writes still work. It uses the shared Git fixture authority and a private, atomically reserved scratch root. The focused command completed **2 passed, 0 failed, 343 filtered out**; its raw local output is `/tmp/velnor-phase0-git-locks-tests.log`.
