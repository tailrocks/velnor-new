# Mise `.miserc` Phase 0 evidence

Status: verified with official Linux x86_64 release binaries. The integration test now supports Linux x86_64 and macOS ARM64 with official asset digest mappings. Actual macOS binary execution and the CI-selected run are pending; this evidence does not claim all-platform completion.

Upstream [jdx/mise#13926](https://github.com/jdx/mise/pull/13926) added the `--no-config` and `MISE_NO_CONFIG=1` guards to `.miserc.toml` discovery. Its merge commit is `dfe74a90b41603625ee6aabecb42f14a1f5eb0f6`. The release/tag sources and official release asset metadata are:

| Release | Published (UTC) | Peeled tag source | Linux x64 executable / tar.gz SHA-256 | macOS ARM64 executable / tar.gz SHA-256 |
| --- | --- | --- | --- | --- |
| [v2026.10.1](https://github.com/jdx/mise/releases/tag/v2026.10.1), affected | 2026-10-03 14:12:48 | `050ce5a20287a0aafd872b1191699a5fdafff5ac` | `31e6859cf639ed4594906da3fcd0fe2055e9daddae75e9786dbe50b3fb3c0f4a` / `9b92aa39b8fde54b28c8f974a68f2501925a1523d6c05a52719145df3acdd75a` | `d225d1c8ef2934a86be93692a19365fb1df1cd6958a0af7b85909514e0c608a7` / `19b0ace2ffe420555d277c223f6eef252df3909f018c4faf1adf4a179b67c35a` |
| [v2026.10.2](https://github.com/jdx/mise/releases/tag/v2026.10.2), fixed | 2026-10-04 12:31:22 | `44ea2537166efbe21b19d808355d9914e830941a` | `8f5f6660336f572830e33cd9b378d3131e529a0d4c4f0c553776be90a1ba302a` / `79a2bf0ffc9b8a9a6391344e875b3c3679c15053fda3e8728ddf1790d63db788` | `66d49acecca413c8b334922584982a4907a10588912829873d6c55d0c6d42612` / `a3f67ff009f1436013eee37864c1c1855d9d9a9e4982a660b6495589635384e7` |

The harness requires an absolute regular executable, hashes its bytes, and runs `mise version` before behavioral assertions. On Linux x86_64, observed executable hashes matched the official standalone asset SHA-256 values above. The fixed v2026.10.2 executable was extracted from the official Linux tarball after checking its official asset digest. The macOS ARM64 hashes are official release API asset digests used by the test map; that platform has not yet executed the binaries.

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

Result: **4 passed, 0 failed, 0 ignored, 342 filtered out** after adding the ordinary borrowed-stage parity test. The raw Linux output is `/tmp/velnor-phase0-miserc-tests-macos-repair.log`. The macOS-capable source retains the same four ignored release tests. The local Rust tool installation has no `cargo-nextest` command, so no local Nextest execution is claimed. The CI route uses Nextest's `--run-ignored only` spelling and must hard-fail if its pinned binary setup is absent. macOS and the CI-selected run remain pending evidence.

The ordinary `env_cleared_child_keeps_the_validated_temp_root` test launches a nested test harness with a cleared environment, passes the already validated canonical temp root as `TMPDIR`, and exercises the borrowed fixture's direct-child and marker checks. The production-boundary and Git nested children likewise receive that canonical parent temp root explicitly, so `temp_dir()` resolves to the same root under both normal Unix temp layouts.

The same existing integration binary also contains the generic optional-lock regression port. It verifies that the production `GitRequest`/`IsolatedCommand` overrides hostile parent `GIT_OPTIONAL_LOCKS=1`, that status does not refresh the index with the effective value `0`, that the direct `1` control does refresh it, and that explicit add, commit, and clone writes still work. It uses the shared Git fixture authority and a private, atomically reserved scratch root. The focused command completed **2 passed, 0 failed, 344 filtered out**; after explicit temp-root propagation its raw Linux output is `/tmp/velnor-phase0-git-locks-macos-repair.log`.
