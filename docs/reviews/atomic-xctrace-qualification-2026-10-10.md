# Atomic `.github` qualification trace — 2026-10-10

This capture follows the historical `NotFound` assertion failure. It is one
diagnostic invocation, not a qualification rerun or release acceptance.

## Exact capture

- Source worktree: `velnor-new-version-integrity`, commit
  `87f7390abf7272d076f43b7fb75a9218228d14d0`, tree
  `4dcbafe8b41ee8a3b85c1723ec87a3e28a706b69`.
- Test: `impl_generate_p09_atomic::atomic_commit_never_exposes_missing_tree`
  in `crates/velnor-actions-orchestrator/tests/impl_generate_p09_atomic.rs`.
  The exact-filter listing returned one test and zero benchmarks.
- Binary:
  `target/debug/deps/velnor_orchestrator-6877047af38a04ee`, SHA-256
  `cd3964ae299b07034295519970a97403b3c33e164afd2094ce4281bf293762b5`.
- Capture command:
  `xcrun xctrace record --template 'System Trace' --time-limit 15s --output atomic.trace --no-prompt --launch -- <binary> impl_generate_p09_atomic::atomic_commit_never_exposes_missing_tree --exact --nocapture`.
- xctrace log:
  `/private/tmp/velnor-atomic-pr122-87f.tPNG7r/atomic-test-xctrace.log`,
  SHA-256 `e76779cba149ac1be1ec1444f4773a52608790bc77a96de441edf2c6721da572`.
- Exported TOC SHA-256:
  `53255a7ee9379129e94a39d662a12882ef9c1e1a5ab292837f7d0708e53f9df8`.
  Syscall table SHA-256:
  `1bcc9f199643ce53bceac6630318c5da146d72bbcafe6753370fc31d071bf074`.
  Bundle tree SHA-256:
  `64a89eacc54ce925bb1f85dc6ff103ae7b913231c9e26d5dd7d1cf324f1186d6`.

The TOC records macOS 27.0.1, target PID 8058, test arguments exactly as
listed above, duration 1.053431 seconds, and target termination `exit(101)`.
xctrace reported that the target exited and the recording was saved, while its
wrapper returned 54. These are separate exit statuses. The capture did not
retain target stdout or stderr. Installed `xcrun xctrace record` help offers
`--target-stdout <name>` but no target-stderr option; `--target-stdout` was
not used.
The launch command and TOC do not record the target's working directory or
environment. The tool invocation used the source worktree as its working
directory. Test Git helpers clear the environment and then pass through
`PATH`, `SYSTEMROOT`, and `TMPDIR`/`TEMP`/`TMP` with isolated Git settings.
The trace resolves a Git executable under `/Users/donbeave/.local/bin/git` to
Homebrew Git 2.56.0, but neither the launch `PATH` nor a `git --version`
preflight was retained; treat that tool context as unqualified evidence.

## What the trace establishes

For PID 8058, the exported syscall table contains 1,310 rows across two
recorded thread identities (main and the named test-harness thread): 48
`lstat64` calls, 35 `stat64`, 16 `mkdir`, 35 `unlinkat`, and 8 `posix_spawn`.
It contains no calls labeled `rename`, `renameat`, `renameatx_np`, or `rmdir`.
The 18 `lstat64` results with errno 2 have undecoded pointer arguments; they
cannot be tied to the observer's `.github` lookup. The TOC lists only the test
process, not its Git child processes. `VnodeToPaths` associates that process
with its temporary repository, Git executable
`/Users/donbeave/.local/bin/git` (resolving to Homebrew Git 2.56.0), and one
staged `.../.tmpYElrab/.github` tree. Those path records do not bind the
errno-2 calls to that tree.

The test source calls `make_repo`, `prepare`, and an initial `generate` before
it starts the writer and four observers. The export has no observer-thread
identities or rename calls, so failure during setup/initial generation is a
plausible explanation for exit 101. Trace completeness and missing test
stderr prevent concluding that this happened. The run therefore does not
show that the atomic assertion fired, does not attribute the historical
`NotFound`, and cannot clear the failed qualification.

An earlier harmless xctrace smoke capture succeeded without an elevation
prompt and confirmed exportable syscall/VFS tables; its analysis filtered to
the launched target PID. System Trace also contained kernel events, so
collection was not process-only. A prior
DTrace attempt was separately rejected for requiring additional privileges;
that result does not negate this successful xctrace capture or establish that
all tracing methods are unavailable.

## Output-retaining follow-up

A second, separately authorized invocation used the same source and test with
`--target-stdout` and a transparent `exec "$@" 2>&1` launcher. It is a
different diagnostic run, not a qualification rerun or release acceptance.

- Source/head/tree, test, and binary SHA-256 are identical to the first
  capture above. The test filter listed exactly one test.
- The command and declared environment are recorded in
  `/private/tmp/velnor-atomic-pr122-87f-output-retention-20261010/command.txt`
  (SHA-256 `36193e6fef6c8e21214853e42d05ea29dd5fcd87a1a2ef9a50a6fea4431838d1`)
  and `runtime-context.txt` (SHA-256
  `de3937c1b59bef1e281f248adbcdf954f1d8c63d9b5e5d8a690ab1fb03d54a25`).
  The launcher hash is `bcb8f7faee496ed3ac4936b429167f7d267ea76a2782565203159664437edd04`.
- The target output is retained at
  `/private/tmp/velnor-atomic-pr122-87f-output-retention-20261010/target-combined.log`
  (SHA-256 `a7314c3967b2511881bac13bcc54fb47d381907312ffb0a8a80835d479cb7d5d`).
  It reports one test started, then a `Validation` error: `mise` could not
  resolve `zizmor@1.30.1`, fell back to `PATH`, and could not execute
  `zizmor`. The launch did not forward `MISE_DATA_DIR`, and its `PATH` omitted
  the ordinary pinned Zizmor install directory.
  The test result was 0 passed, 1 failed, and 728 filtered out. The test's
  initial `make_repo`/`prepare`/`generate` setup runs before the writer and
  observer threads, so this follow-up failed before the atomic observer.
- That launch used Rust 1.98.1, Mise 2026.10.4, Actionlint 1.7.12, and
  ShellCheck 0.11.0. Its recorded `PATH` omitted the ordinary pinned Zizmor
  install directory, and the command did not forward `MISE_DATA_DIR`; the
  output therefore records a validator-resolution failure in that launch
  context. A later
  read-only preflight using
  `MISE_DATA_DIR=/tmp/velnor-110-mise-data-run2` and
  `MISE_AUTO_INSTALL=false` successfully resolved the ordinary
  `zizmor@1.30.1` selector. This classifies the follow-up's setup failure; it
  does not retroactively classify the first capture, whose target output was
  not retained.
- Before the current-head qualification batch, an isolated tool preflight
  verified Rust/Cargo/Clippy 1.98.1, MBX 1.21.1, Nextest 0.9.148, Mise
  2026.10.4, Actionlint 1.7.12, ShellCheck 0.11.0, Zizmor 1.30.1, cargo-deny
  0.20.2, and Alint 0.17.0. Exact paths, environment, command resolution,
  versions, and no-config/no-env/no-hooks Mise selector checks are recorded in
  `/private/tmp/velnor-atomic-pr122-87f-output-retention-20261010/gate-preflight.log`
  (SHA-256 `d9786a744e5898ff085955df9e44ea17806e7510b3646fc5b8ded9454b9d3570`).
  Nextest 0.9.148 was installed from its pinned Aqua selector into the
  isolated Mise data directory at
  `/tmp/velnor-110-mise-data-run2/installs/aqua-nextest-rs-nextest-cargo-nextest/0.9.148/cargo-nextest`;
  no repository files changed.
- xctrace's wrapper returned 54 while its target returned 101. The exported
  TOC SHA-256 is
  `c977f6edaa1d8e38b748b0182aa0e744297976b8c354c5b27a96f80d71f9ba82`;
  the filtered `VnodeToPaths` export SHA-256 is
  `2bd79840b1d0b450e7b860324142f459b1a3b4c103aeb09efbb2abd9a3f124eb`.
  The export associates the target PID with the exact test binary after the
  shell's `exec`; it does not change the pre-observer failure classification.

Independent review of the objective and contracts found no explicit
acceptance requirement to identify the kernel stage that produced the earlier
`NotFound`. That stage remains a diagnostic unknown, not a separate release
gate. The original failure is still preserved as evidence; the follow-up
neither reproduces nor clears it. PR #122's current-head full qualification is
tracked separately from these diagnostic captures. No publication or consumer
adoption is claimed.
