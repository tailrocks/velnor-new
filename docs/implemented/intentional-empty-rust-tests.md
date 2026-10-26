# Allowing an intentionally empty generated Rust test selection

Use this procedure only when an automatic package Nextest task has an applicable
Cargo test target but its configured selection can intentionally contain no
matching tests. The setting is repository-wide for generated Rust package tasks.

This option requires `test_runner = "cargo_nextest"`; it does not change the
selected test runner.

Contract: [Rust test policy](../proposed/rust-test-policy.md) and
[task execution contract](../proposed/task-execution-contract.md).

## Decide whether the option applies

Leave the option unset for the normal fail-on-empty behavior. An omitted option
defaults to `false`; generated unsharded Nextest tasks use `--no-tests fail`.

Cargo metadata can also show that a package has no applicable test target. In
that case Velnor skips the task and reports `valid_no_test_targets`; Nextest does
not run. This differs from an applicable target whose selected suite discovers
zero tests. The option does not register disabled Cargo targets or create tasks.

Hash-sharded tasks with more than one shard already use
`--no-tests warn --partition hash:<index>/<count>` so an empty shard can pass.
This option does not change that command. Named suites under
`[[stacks.rust.suites]]` use separate configuration and are not changed by
this option. Their partitioning and filtering remain governed by their own
suite entries.

Enable the option only when allowing empty selections is intentional across
the repository's generated package tasks. Review each applicable package and
configuration before enabling it.

## Configure

Add this field to the existing Rust stack table in `.velnor/config.toml`:

```toml
[stacks.rust]
allow_empty_test_suite = true
```

Do not add a duplicate `[stacks.rust]` table. Regenerate the workflow and inspect
the generated Nextest commands. Unsharded package tasks should use
`--no-tests warn`; sharded commands and the task set should remain unchanged.

## Verify generated commands and runtime behavior

Use a temporary consumer fixture with an applicable test target and an empty
Nextest selection. First omit the setting, then enable it and regenerate. Table
entries are abbreviated command examples, not commands to copy. Execute the
complete Nextest command emitted for each case, preserving its Mise invocation,
profile, offline setting, features, manifest and package selection, and all
generated arguments. Do not reconstruct it from these examples. Expected exit
codes are acceptance targets to verify, not recorded results.

| Case | Abbreviated generated-command example | Expected exit |
| --- | --- | ---: |
| Option omitted; empty unsharded selection | `cargo nextest run --package <package> --locked --no-tests fail` | 4 |
| Option enabled; empty unsharded selection | `cargo nextest run --package <package> --locked --no-tests warn` | 0 |
| Option enabled; real test fails | `cargo nextest run --package <package> --locked --no-tests warn` | 100 |

For the third case, confirm the task selects and executes a real test that
fails; an empty selection alone does not verify failure propagation.
