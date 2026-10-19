# Parallel Cargo target seeding

Rust consumers can explicitly enable initialization of missing parallel check
targets:

```toml
[stacks.rust]
seed_parallel_targets = true
```

The boolean defaults to `false`; an absent or false setting adds no preparation
step and is omitted from serialized configuration. It is a Rust stack option,
not a generic workflow option. Enabling it does not enable Rust discovery or
create an obligation that the repository did not already declare.

Each eligible package/configuration still owns one job. Its existing Build test
executables obligation finishes first, then a sequential seed barrier runs
before the native parallel check group. Format, Clippy, documentation, tests and
doctests retain their commands, conditions, reports and target ownership. The
existing `wait-all` still requires every parallel check to succeed. Required
retains the complete job and report inventory. The barrier does not extend the
job timeout; the default remains five minutes, including copying and cleanup.

The helper takes the existing workspace-relative Cargo target root and only
the declared `velnor-clippy` and `velnor-doc` lanes. It initializes missing lanes
only. Any existing real lane directory, including an empty directory, is
preserved. A lone sequential check, a graph without the build producer, or a
parallel group without either dedicated lane receives no seed step.

For each missing lane it copies the complete baseline `debug` tree and optional
`.rustc_info.json`. It excludes `.cargo-lock`, preserves regular-file bytes and
modification times, and never edits Cargo fingerprints. Copies use independent
files, not hardlinks or symlinks. Completed private staging directories enter
through atomic exclusive rename; an existing destination is never replaced.
Checks start only after the barrier returns and continue to use separate
writable targets.

The fixed resource admission limits are a 20-second inventory, copy and admission
deadline, 8 GiB of
combined copied bytes across missing lanes, 64,000 debug-tree inventory entries,
directory depth 64,
and a 2 GiB free-space reserve beyond the admitted copies. Deadline checks share
one clock across inventory and copying. Missing baselines and exhausted optional
time, byte, entry or disk budgets are logged and owned staging is cleaned. Lanes
without an admitted copy run cold; an already admitted complete lane is retained.
Unsupported atomic rename is also an optional cold fallback. Cleanup remains
inside the job timeout rather than receiving a separate timeout allowance.
These limits do not grant another job timeout or omit any check.

Unsafe paths, symlinks, special files, unexpected identity changes, source growth
or conflicting destination admission fail the preparation step. Cleanup removes
only identity-owned staging; it does not remove an unknown replacement or an
existing lane. The generated fixed helper step uses the existing credential
scrubbing boundary and the hosted runner's Python runtime.

The implementation and its filesystem regressions support a bounded opt-in
experiment. Local measurements show partial Cargo reuse and a material disk
copy cost; they do not prove that a Linux hosted job with MBX will finish within
five minutes. Acceptance requires an actual representative hosted run with all
quality checks, reports, cache lifecycle and the unchanged five-minute ceiling.
The option remains off unless the consumer explicitly selects it.
