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

No second atomic test capture was run. The historical `NotFound` remains
unresolved; PR #122 has no release qualification or acceptance from this
trace, and no publication or consumer adoption is claimed.

If a further capture is separately authorized, preserve the exact runtime
binary and record its pinned `PATH`, environment, and working directory before
launch. Installed xctrace help documents `--target-stdout <name>` but has no
target-stderr option. A transparent launcher can `exec` the same binary and
arguments with stderr redirected to stdout, while xctrace saves target stdout
to a file; record that launcher and redirection as part of the capture method.
This was not done for the capture above and is not a test result.
