# Exact synchronous registry fixture

Scope: `exact_synchronous_fixture_v1`. Inputs only; no native execution claim.

The first-party Rust package lives at
`crates/velnor-actions-mise/tests/fixtures/mbx-synchronous/registry-fixture`.
It is a standalone edition 2024, MSRV 1.98 library with an explicit resolver 3
workspace boundary. `Cargo.lock.fixture` preserves the reviewed Cargo lock
bytes under a name that does not become a nested Cargo lockfile. The harness
copies the fixture into each isolated Cargo root and renames that file to
`Cargo.lock` before any declared `--locked` build. Its lock digest remains
pinned in this manifest. The lockfile was constructed from the existing
repository's genuine registry `itoa 1.0.18`
package entry without running Cargo. The local cached `.crate` checksum matched
that entry; all 14 archive files matched the extracted registry source. The
manifest pins every archive file, the extraction marker, and every fixture file.
No archive is vendored and no path dependency substitutes for the registry.

Reviewed compiled dependency source: `src/lib.rs` and `src/u128_ext.rs`.
`build = false`, no proc macro, `no-panic` disabled, and no subprocess API.
Dependency development targets/dependencies are excluded. Upstream itoa contains
unsafe code; fixture code forbids unsafe. Only `cargo build/check --lib` qualify;
this fixture supplies no application, native linker, build-script, or proc-macro
lifetime proof. Its test executes separately from measured commands.

`manifest.json` declares exact Cargo argument vectors. The execution owner must
bind the actual owned MBX command wrapper, absolute tools, controlled environment,
configuration discovery, compiler/sysroot, host, and raw execution artifacts.
Reject source replacement, feature overrides, extra source inputs, compiler
wrappers, loader injection, custom target specs, and Rust flags injecting codegen
backends/link arguments. Running outside this repository in an isolated copy
avoids inheriting repository Cargo configuration; isolation still needs evidence.
Executable names and hashes alone prove no compiler process lifetime. The exact
Rustc/Cargo execution owner requires independent source qualification.

`bind_inputs.py` verifies the reviewed manifest digest, exact fixture inventory,
archive checksum, full extracted source inventory, and absence of extra inputs.
Supply an independently reviewed manifest SHA-256; accepting a hash newly made
from altered inputs would defeat the review binding. It optionally hashes an
opaque owner execution record and every raw artifact into a receipt with status
`fixture-input-binding-only` and `native_authority: null`. It never parses report
booleans or infers qualification. The external qualification gate joins these
bytes and trusted source-publication receipts with the owning session's sealed
native artifacts. It cannot reconstruct native authority from JSON. Unknown
evidence remains unknown. Supply `--fixture-root` with the absolute canonical
executed copy to verify that copy before and after use; omit it to verify the
committed fixture. Symlink roots/ancestors and path aliases are rejected. Keep
Cargo target outputs outside the fixture so its exact source inventory stays
unchanged. The optional receipt records the verified fixture root.

`copy_fixture` creates a writable private workspace copy: directories use mode
`0700` and files use `0600`. `shutil.copytree` preserves readonly source modes,
so the copy is normalized before `Cargo.lock.fixture` is renamed to `Cargo.lock`.
`test_mutable_workspace.py` checks this with a temporary readonly copy of the
reviewed fixture. Registry seed tests use synthetic readonly 1.0.17/1.0.18
inputs to check the current byte-copy routes and source preservation; they make
no claim about writable registry copies or a historical 35-file Cargo home.

`test_observer_artifact_closure.py` is excluded from this harness. It imported
an untracked V13 driver and V12 retained run, then asserted `stage_artifacts`,
`staged_inventory`, and public dependency-owner APIs absent from the checked-in
driver and qualification contract. Current observer and public-artifact
selection behavior remains covered by `test_negative_v2.py`,
`test_negative_pipeline_v2.py`, and `test_run_negative_t08_v2.py`; artifact
closure staging is not claimed.

Execution queue: review inputs, copy exact fixture into a clean isolated root,
verify source/manifest bytes before and after, run the declared commands through
the actual owned MBX route, retain complete evidence, and let the qualification
gate evaluate the typed proof. Warm registry reuse requires a second independent
target directory while preserving the owned cache namespace and exact inputs.
No Cargo, index, Git, compiler, or measured task mutation occurred during fixture
construction.

## Local execution receipts

`run.py` observes three isolated roots. `run_same_root.py` instead requires
`--active-root` and recreates that same absolute root for each state. It records
exclusive allocation, pristine directories, source inventories, raw byte copies,
and destruction before the next state. Only immutable supported MBX export
bundles cross states. Its separate `--output` directory retains all evidence.

`join_same_root.py` checks original-to-retained byte witnesses, complete archives,
physical lifecycle receipts, and released ownership. Original native report paths
remain unchanged. Both joins retain Unknown native/compiler authority; local
same-root observations cannot qualify fresh hosted runners or portable restore.
