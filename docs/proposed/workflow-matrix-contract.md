# Velnor workflow matrix contract

Status: normative companion to [the workflow contract](workflow-contract.md).

## Generic matrix contract and V1 Rust payload

`plan` MUST emit a JSON array named `matrix.include`. Each entry MUST contain the generic fields below;
adapter metadata and task references are opaque to the planner and renderer. The example is V1's Rust payload;
future stack adapters define their own versioned metadata:

```json
{
  "id": "stack:rust|task:stack/rust/crates/velnor-actions-contract/validation/default",
  "matrix_key": "m-<16 lowercase hex characters>",
  "stack_id": "rust",
  "task_id": "stack/rust/crates/velnor-actions-contract/validation/default",
  "adapter_metadata": {
    "package_id": "path+file:///repo#velnor-actions-contract@0.1.0",
    "package_name": "velnor-actions-contract",
    "manifest": "crates/velnor-actions-contract/Cargo.toml",
    "workspace": ".",
    "features": [],
    "target": "host",
    "cargo_profile": "test",
    "compile_driver": "mbx",
    "test_runner": "cargo_nextest",
    "doctests": true
  },
  "execute_task_ids": {
    "clippy": "stack/rust/crates/velnor-actions-contract/clippy/default",
    "test_build": "stack/rust/crates/velnor-actions-contract/test-build/default",
    "test_inventory": "stack/rust/crates/velnor-actions-contract/test-inventory/default",
    "test_run": ["stack/rust/crates/velnor-actions-contract/test-run/default/shard-1-of-1"],
    "doctest": "stack/rust/crates/velnor-actions-contract/doctest/default"
  },
  "input_digest": "b3-<64 lowercase hex characters>",
  "report_id": "report-r123-a1-m-<16 lowercase hex characters>",
  "artifact_id": "velnor-matrix-r123-a1-m-<16 lowercase hex characters>"
}
```

The example's `mbx`/`cargo_nextest` values describe the Velnor repository's own
dogfood profile. Every generated entry MUST carry the detected
`compile_driver`, `test_runner`, and evidence IDs. For Cargo-test profiles,
omit Nextest build/inventory/shard task IDs and emit one `test` obligation;
the renderer MUST NOT invent Nextest work. Cargo-doctest remains separate in
either profile.

`id` MUST be stable for the same stack, task group, configuration, and adapter inputs. `stack_id` MUST be a
registered detector ID. `task_id` is the stable matrix task-group ID; individual executable obligations remain
in `execute_task_ids`. It MUST contain only lowercase ASCII letters, digits, `:`, `|`, `/`, `.`, `-`, and `_`.
`matrix_key` is `m-` followed by the first 16 lowercase hexadecimal characters of BLAKE3 over the UTF-8 bytes
of `id`. A collision is a planning error. Adapter metadata is owned by the selected stack adapter; the generic
orchestrator MUST preserve it without interpreting stack-specific fields. Matrix entries MUST be sorted by
`id`; duplicate IDs are a planning error.

`report_id` is `report-<run-key>-<matrix-key>`. `artifact_id` is `velnor-matrix-<run-key>-<matrix-key>`. These
values are derived, not chosen by repository input. The plan artifact is `velnor-plan-<run-key>`. The optional
candidate artifact is `velnor-candidate-<run-key>-<target-key>`, where `target-key` is the lowercase target
triple with non-alphanumeric runs replaced by `-`. Artifact names MUST be unique within the run and MUST use
only the derived values above.

The default matrix policy is one entry per selected stack task-group/configuration with at least one `execute`
obligation. Its `execute_task_ids` contains only obligations that this matrix entry must run; `test_run` is an
array of shard task IDs, including `shard-1-of-1` when not sharded. The complete plan retains covered/reused
obligations separately. The V1 Rust adapter MUST NOT generate `--all-features` configurations. Cross-package
or cross-component integration tasks MUST be represented by explicit matrix entries. An intentionally empty
test target MUST remain visible in the plan.

The plan file written by `plan` MUST be `$RUNNER_TEMP/velnor/<run-key>/plan.json` and MUST contain the
same `matrix.include` array that is sent through `GITHUB_OUTPUT`. Its schema-1 shape is defined by the
[architecture contract](architecture.md); baseline and obligation fields use the exact proof format in the
[parallelism and affected-work contract](parallelism-and-selection-contract.md).

`plan_id` is `plan-<run-key>`. `detections`, `obligations`, `matrix.include`, and `task_ids` MUST be sorted as
defined by the architecture contract. `task_ids` contains every obligation; matrix `execute_task_ids` contains
only `execute` decisions. A baseline miss MUST populate the reason and broaden execution. A baseline proof
MUST identify the exact manifest/run/task/input digests. The plan job MUST publish `plan_id`, `run_key`, and
compact `matrix` JSON as named step outputs; the task job MUST consume that exact JSON and MUST NOT rediscover
stacks. The plan artifact MUST contain `plan.json` and `matrix.json`, where `matrix.json` is exactly
`{"include": [...]}`; the final job MUST validate byte-for-byte agreement after canonical JSON encoding.
