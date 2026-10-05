# P00 exact-head F2b failure: repro record

Repo-local record of the red test that opened PR #1 remediation. The deleted
comment-prose test is NOT resurrected; this file preserves its failing output
and the exact pinned toolchain so the defect stays reproducible on paper.

## Identity

- Exact head: `8ccc60c506a232ff6de524d24afea463fbdb95a4`
  (`docs/velnor-actions-spec`, PR tailrocks/velnor-new#1)
- Actions run: `36617447350` (FAILURE, 2026-09-29)
- Failing job: `109575245197` (Rust-adapter Nextest task)
- Failing test: `impl_rust_f2b::alint_config_holds_generic_rules_only`
  (`crates/velnor-actions-rust/tests/impl_rust_f2b.rs:104`)
- Failing assertion: `text.contains("Generic file/path placement")`
  against `.alint.yml`, whose line 2 reads
  `Generic file/path requirements ...` — comment wording drifted,
  the assertion did not.

## Verbatim CI failure (job 109575245197)

```text
FAIL [   0.004s] ( 60/109) velnor-actions-rust::velnor_rust impl_rust_f2b::alint_config_holds_generic_rules_only
    test impl_rust_f2b::alint_config_holds_generic_rules_only ... FAILED
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 108 filtered out; finished in 0.00s
    thread 'impl_rust_f2b::alint_config_holds_generic_rules_only' (3727) panicked at crates/velnor-actions-rust/tests/impl_rust_f2b.rs:104:5:
    assertion failed: text.contains("Generic file/path placement")
    Summary [   0.077s] 63/109 tests run: 62 passed, 1 failed, 0 skipped
    warning: 46/109 tests were not run due to test failure (run with --no-fail-fast to run all tests, or run with --max-fail)
    error: test run failed
```

Fail-fast counts: registered 109, run 63, passed 62, failed 1, not run 46.
Exit code 100.

## Exact CI command and pinned toolchain

```text
mise --no-config --no-env --no-hooks exec rust@1.98.1 mr-boxington@1.19.0 aqua:nextest-rs/nextest/cargo-nextest@0.9.146 -- mbx nextest run --locked --offline --manifest-path crates/velnor-actions-rust/Cargo.toml --package velnor-actions-rust --no-tests fail
```

| Tool | Pinned (CI-effective) | Source |
|---|---|---|
| rust | 1.98.1 | mise.toml, catalog `RUST_VERSION` |
| mr-boxington (mbx) | 1.19.0 invoked | catalog `MR_BOXINGTON_VERSION` |
| cargo-nextest | 0.9.146 | mise.toml aqua pin, catalog `NEXTEST_VERSION` |
| actionlint / shellcheck | 1.7.12 / 0.11.0 | install line |
| git | 2.55.0 | runner log |
| runner | x86_64 Linux (`RUST_TARGET_TRIPLE`) | catalog |

Known pin gap (P07, not P00): the Mr. Boxington action reported setting up
MBX 1.21.0 while the invocation pins 1.19.0. Pinning the action SHA did not
pin its installed executable.

## Root cause and fix

Two defects shared one test: (1) repository policy was "proven" by comment
prose instead of parsed configuration; (2) a rule-kind whitelist
(`file_exists`, `file_absent`, `file_max_lines`) rejected newly supported
kinds such as the `command` rules already in use. Fix `1748495` deleted the
test from the Rust adapter and moved verification to the CLI repo-policy
layer (`alint_miniyaml` structural parse + per-rule pass/fail fixtures).

## Local repro (independent, 2026-09-30)

Detached worktree at `8ccc60c`, local cargo 1.98.1 (matches pin):

```text
$ cargo test --locked --offline -p velnor-actions-rust --test velnor_rust impl_rust_f2b::alint_config_holds_generic_rules_only
test impl_rust_f2b::alint_config_holds_generic_rules_only ... FAILED
thread '...' panicked at crates/velnor-actions-rust/tests/impl_rust_f2b.rs:104:5:
assertion failed: text.contains("Generic file/path placement")
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 108 filtered out
```

Full collection at the exact head (no fail-fast):

```text
$ cargo test --locked --offline -p velnor-actions-rust --test velnor_rust
test result: FAILED. 108 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

Registered 109, run 109, passed 108, failed 1 (F2b only), not run 0:
fail-fast hid 46 passes and zero additional failures.

## P00-6 full-collection counts at 2c02db9 + residuals

`cargo nextest run --locked --offline -p <pkg> --no-fail-fast`, default
profile (CI used no `--profile` flag either). Local nextest 0.9.143 vs
pinned 0.9.146, local mise 2026.9.16 (matches), rustc 1.98.1 (matches),
macos-arm64 host (CI is x86_64 Linux).

| Suite | Registered | Run | Passed | Failed | Not run |
|---|---|---|---|---|---|
| velnor-actions-rust | 131 | 131 | 131 | 0 | 0 |
| velnor-actions-cli | 170 | 170 | 170 | 0 | 0 |

Both exit 0. The CLI count includes the P00-5 test below and excludes the
removed exact-duplicate `alint_config_semantic_policy` (same assertion as
`p11_alint::alint_extended_policy_holds` in the same binary).

## Residual regression tests

- P00-5 `impl_repo_policy::alint_comment_only_edits_keep_verdicts`: live
  `.alint.yml` plus all 7 pass fixtures wrapped in comment mutations pass
  semantic policy; pass fixtures also carry trailing `#` probes exercised
  by the existing pass/fail test. A semantic mutation (version bump) fails
  the test, proving it is load-bearing.
- P02-6 `impl_select_base::{build,dev,optional,target-cfg}_only_edge_...`:
  per-kind reverse-closure selection through `plan_pr`; an empty section
  fails, proving the affected-reason assertion is load-bearing.
- P02-7 `impl_select::md_in_package_selects_narrowly`: in-package Markdown
  selects its owner narrowly instead of broadening.
