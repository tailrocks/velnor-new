# Isolated PR39+098 matrix-tools resolution

Candidate root: `/root/.cache/velnor-task-targets/20261005/mise-phase0-pr39-098-route-candidate`
Source snapshot identity: `/root/.cache/velnor-task-targets/20261005/pr39-098-stage0-snapshot-20261005/identity.txt` SHA-256 `e2404013fd000f3d01a998fc21bbd022fbcfccdaf3b435df1a50ceb6e3316b70`. This is a stage-0 PR39+098 proposal, not current live main or the steward worktree.

## Preimages and postimages

`crates/velnor-actions-orchestrator/src/matrix_tools.rs`:
- Merge base `703d1b84d9f9e0246a52e3989bf3b78a13bbbf05`, blob `815bbc9159be2014706e70d1492121931a4cdd5f`, SHA-256 `cc73c60aa149e78f397e41a8e5d8d2bdd3cb38ed9c6e372d4587d25847ea560e`.
- PR stage 2 `39d489229747342043de979e0f3e2c95d8726aa4`, blob `e3efe0c4bcaf9e8090ede93f9567535424e26731`, SHA-256 `99c55397ad521c8e1aa3cb457efb455447426a17d707ca636fd4f841f2d884b4`.
- Main stage 3 `098be4ad61e614fa4ddfb30f7e7148e470974597`, blob `805ec4ab6a8bc90bb137e1574cb68c4c9cad649a`, SHA-256 `8e4ec684a52fff605e6e904c696adcafc8ad472ddab99cc63cd4235094462ce6`.
- Resolved postimage SHA-256 `bd209a408520426504a36f5ca355d59b59f81ed519e136feff3b9e1b71ede2f7`.

`crates/velnor-actions-orchestrator/src/crate_jobs.rs`:
- Stage-0 source-only preimage SHA-256 `fd298a54ba6bf9d328b3ef7adad5f99b5e5fea97e6c8c595c784f9218a7d7a5b`.
- Resolved postimage SHA-256 `871ea76e0f03170db8d5e346354e568bfb8760a6860155b7e382f8a007cc3718`.

`crates/velnor-actions-orchestrator/src/wire_w1_tests.rs`:
- Stage-0 source-only preimage SHA-256 `ca664e0a799924c3b33d53399d3eba342a75cb4915abfacc18510d71e07ec1cf`.
- Resolved postimage SHA-256 `fdf601a4976efea6b4e0024c688678eec9c1e476c6a1578cfbc8cc7b45cddbe6`.

The final source preserves the PR's explicit `use_mbx` task-driver role alongside Rust and task-driven OpenTofu. It retains main's suite registry and parity check, exact Velnor validator/OpenTofu policies, opaque-consumer validator behavior, ConsumerV1's no-OpenTofu rule, and fail-closed errors for unknown or mismatched suites. `prepare_install_opentofu` is no longer needed because task-driver OpenTofu and audited-suite OpenTofu are passed independently and combined at `prepare_crate_tools_step`.

## Raw diffs

- `matrix_tools.stage2-to-final.patch` SHA-256 `3d64063be1572ec3063b1702c15259ad1575c1528336b1732e4fc38cd0499bc6`.
- `matrix_tools.stage3-to-final.patch` SHA-256 `1f82c9d7ce83584dbe7a95a00af3ea5e5ea13e3fcd843dfc023ab3a143aa8877`.
- `crate_jobs.stage0-to-final.patch` SHA-256 `bf3aec14b210de47c07eba38c40ecfd9b0fc3e659833c6e2a5df64789bec4de7`.
- `wire_w1_tests.stage0-to-final.patch` SHA-256 `8935189955e348c563da97a791171bdc283ea0adfa3f56a116a3494f0a79d3a4`.

## Validation

The harness was built with Rust 1.98.1 by `mise exec rust@1.98.1 -- cargo test --locked --offline --manifest-path crates/velnor-actions-orchestrator/Cargo.toml --package velnor-actions-orchestrator --lib phase0 --jobs 2 -- --test-threads=1`. Binary: `/root/.cache/velnor-task-targets/20261005/mise-phase0-route-target/debug/deps/velnor_actions_orchestrator-4f04318694f8a7e8`, SHA-256 `0c58f86182cf40621e0aaf232300c73f59ba344080e83327bc3beaf7d0dfe794`. Build log: `/root/.cache/velnor-task-targets/20261005/mise-phase0-route-unit-fix.log`, SHA-256 `e19846490f3ab60e34e80cebe0a2975ff0f7707540b2d8847d78ac7bbfd74df4`.

Focused harness invocations and raw logs:
- `workflow::wire_w1::wire_w1_tests::crate_`: 3 passed, 0 failed; `/root/.cache/velnor-task-targets/20261005/mise-phase0-matrix-focused.log`, SHA-256 `01b2eb25d2cca8e4d1c681cd70d8d85f35634171dda7c9fdc87b55fdafdc12fd`.
- `matrix_step::`: 28 passed, 0 failed; `/root/.cache/velnor-task-targets/20261005/mise-phase0-matrix-policy.log`, SHA-256 `89aec375d165b908f1cac44c777f5c6398fd9f1ba6a47bd2b0c668963063671e`.
- `crate_jobs::crate_jobs_tests::mbx_tests::drivers_follow_per_crate_selection --exact`: 1 passed, 0 failed; `/root/.cache/velnor-task-targets/20261005/mise-phase0-matrix-mbx-role.log`, SHA-256 `ad9c4faca11655e579d7e0b66fbc9f45dba30da87971d1514b4d10bb82155a36`.

The three matrix/caller/test paths retained these hashes at harness build and were unchanged afterward. Later route edits affected only workflow/Phase0 paths. No Git index, refs, steward worktree, generated outputs, or shared files were changed.
