# Native supplied-document semver source extension

## Source identity

The official `cargo-semver-checks` v0.50.0 source is commit
`4297e8b5f6306531375ba2ba332171e5792b4c38`, tree
`4f640b40228b0162141eb1d10961b4c1b5029189`.
The isolated source extension is DCO/coauthored commit
`583dddce84706786fc54c41a2c768c28a09c65fd`, tree
`b0f6ea8b85ac0ed288fc29996e441aaa61bbab48`.

| Artifact | SHA256 |
|---|---|
| Full binary base patch | `ae016d81b76419c9d96499c4f527a69867faa776884891d7a391290246708aef` |
| Exact committed source tar | `38573667b13c541e368395259545be0be8f6858ada3b9c158ba93d8633b38c11` |
| Cargo.lock | `34280e954f1a748a60d7338272d76e95706df89771efc5a4f9938e53cd40609b` |
| Apache license | `91687e47b87fadb95cd01f7a85028c6ba4fab03bddb7269d581ae7dd43de5b03` |
| MIT license | `23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3` |

Source tar size: 7,290,880 bytes. These are local source-owner artifacts;
immutable publication and three-host runtime qualification remain pending.

The source capsule preserves raw official and both owned commits. Independent
reconstruction applies the full-index binary patch to the exact official base
and obtains the owned tree. Native Git archive with `tar.umask=022` contains
1,814 files and 1,391 directories with exact blobs and Git modes, using only
0644/0755. Adapter crate checksums and Apache-2.0 OR MIT licensing were checked;
all 412 locked package records remain unchanged.
The final source capsule manifest SHA256 is
`13139d97abbd474aa0c92c533b81516c0c5e9376dad7d0d88b8a8f8a14b5c71a`;
independent final review SHA256 is
`81289a4b235426be284ffdff5f6b64e7f6d81da91c18a12f290ed48de7c06a4a`.
The review passed source integrity and local build/semantic binding. The capsule
contains source and local proof receipts, with no runtime binary asset. Source
publication still requires the parent-owned remote staging window.

## Publication trigger review

The final source commit restricts actionlint push events to `main`, preserving
its path filter and pull request behavior. An independent review inspected all
eight workflows: both push workflows select only `main`; no tag,
`pull_request_target`, `workflow_run`, or release event triggers exist. Thus a
non-main source branch/tag push has no matching push workflow. Pull request,
schedule, manual dispatch and reusable workflow behavior remains present.

## Native architecture

Official supplied-file mode loads `PackageStorage` without Cargo package data.
The three Major/Deny feature queries can execute with no feature evidence.
The 58 skipped lints under an explicit Minor release are native bump/Allow
filtering; they are not 58 missing metadata checks.

The extension adds the dedicated `cargo-semver-checks-owned` binary. Its exact
banner is `cargo-semver-checks-owned 0.50.0 velnor-supplied-v1`; the banner alone
never authenticates source. Closed schema-v1 `plan REQUEST.json` and
`compare REQUEST.json` commands use these existing native owners:

1. `CrateSource::{ManifestPath,Registry}` and `generate_data_request` select
   current/registry features, optional and target-only dependencies, `dep:`
   suppression, baseline filtering, ordering, deduplication and defaults.
2. `RustdocBuildEnvironment` resolves actual configuration, target, rustflags,
   rustdocflags and rustdoc identity. `determine_rustdoc_dir` resolves the native
   output location, including the actual Cargo config probe when target is absent.
3. Exact native Cargo `Package` selection constructs versioned
   `PackageStorage::from_rustdoc_and_package` directly for formats 57/60/61.
4. The unchanged native workspace/package override helper reads actual
   standalone manifests, including `lints.workspace` inheritance. The unchanged
   `run_check_release` applies query filtering, lint severity, required update
   and report policy.
5. Comparison captures actual native report stdout with color disabled. The JSON
   report projection preserves exact native enum names and counts. Native exits
   are 0 for success, 100 for incompatible API, 101 for execution failure.

Both doc-generation arms must use the same process Cargo configuration context
as the native checker: plan and current/baseline Cargo generation share the
current source root as invocation cwd, with explicit manifest paths selecting
actual side source. Separate baseline invocation cwd would change Cargo config
semantics.

The authenticated SDK producer owns source/lock/config bytes, genuine fresh
Cargo metadata, registry selection/index/archive provenance, feature/target
plan admission, rustdoc generation and before/after evidence. The checker API
computes native policy from supplied typed context; it does not authenticate
arbitrary externally supplied Cargo feature graphs. No Python Cargo
normalization or duplicate semver policy was introduced.

## Measured local proof

Absolute Rust/Cargo/rustdoc 1.98.1 tools compiled the exact final committed tree with
`cargo build --locked --offline --bin cargo-semver-checks-owned -j 2` in its
isolated source target. The qualified Python driver used version 3.14.8 and an
explicit environment with no inherited Git capabilities. The build exited 0
in 1.77 seconds using normal Cargo fingerprints and recorded unchanged complete source bytes/modes, tools,
commit and tree. Its local macOS development binary SHA256 is
`c87864db035e759be6d2e9845c0eb1baa7b026c186dca427950d11bf4abf56aa`.
Earlier builds with in-flight source changes remain unqualified evidence.
The final build and semantic checks each captured direct source/root metadata
guards before and after execution and proved those bytes unchanged. Root HEAD
was `8cba870d41f00be145ff6ddb3420926078d85451`; root index SHA256 was
`fddc02fa2bf803b94b0c58fb895e86aa1a1959dcea6fea8027fed8f1ecf11025`.
Earlier build records retain their separate missing root pre-guard limitation.

An independent verifier executed 22 native semantic cases: all/default/none/
heuristic features, sorted explicit deduplication, missing baseline filtering,
optional dependencies, `dep:` suppression, target-only optional dependencies,
feature removal, transitive implication removal/rerouting, transitive default
removal, private-feature filtering, both inheritance mechanisms, package
precedence, required-update overrides and four forged package selectors.
All passed again on the committed-tree binary; source/rustdoc/binary inputs
stayed unchanged. Final invocation/guard summary SHA256:
`a233b41a57eb880df755b67832ac1707ab3fd38368f8111fafd19a55a2a005f1`.
The synthetic metadata fixtures qualify native semantics; they do not grant
metadata provenance authority.

The same binary also consumed genuine published `is_terminal_polyfill` 1.70.2
API documentation against existing versioned diagnostic 1.71.0 docs with no
Cargo executable in PATH: compatible addition exited 0; public trait removal
exited 100, reported `trait_missing`, and preserved 505 bytes of actual native
report stdout. Both performed 196 checks and skipped 58 by native policy.

## Remaining qualification

Genuine authenticated SDK producer execution, full release-plz native update integration, three supported hosts,
immutable source/runtime publication and hosted performance remain separate
pending gates. Official stock v0.50.0 receipts do not qualify this extension.
