# Testing the generator

| Layer | Proves | Where |
|---|---|---|
| Rule tests | One function: this input gives this output | Next to the code, in each crate |
| Golden tests | A whole sample repository gives exactly the expected files | `crates/velnor-gen/tests/it/golden.rs` |
| Helper tests | `velnor-ci` commands against temporary folders and recorded API responses | `ci-helper/` |
| Real runs | The generated workflow works on GitHub | `velnor-lab` nightly and this repository's CI |

## Rules

- Assert what a user of the generator can observe: the written files, the selection, an error's type and path. Never the wording of a message, the length of a list, or a private structure.
- A test name states the behavior, for example `docs_only_change_selects_nothing`.
- A rule test is a table of inputs and outputs, not one test per case.
- No test starts a program or touches the network. The `io` module is tested by the real runs.
- One behavior is tested at one layer.
- A test that runs longer than 2 s fails the suite (`.config/nextest.toml`).

## Samples

A sample is a folder `samples/<name>/` with `view/` (the files the generator reads), `manifests.txt` (the tracked `Cargo.toml` and `Cargo.lock` paths), `cargo-metadata/<root>.json` (recorded output) and `expected/` (the files the generator must write).

Record a sample from a real checkout. Never type one by hand.

```sh
cd generator && cargo run -p velnor-gen -- --root .. record-sample <checkout> <name>
```

Add one sample for each behavior a change introduces, not one for each case. After a change of the output, rewrite `expected/` and review its diff in the pull request, because that diff is the review of what users get.

```sh
cd generator && UPDATE_GOLDENS=1 cargo nextest run
```

The check `generator-samples` lints every `expected/` workflow with `jactionlint`.
