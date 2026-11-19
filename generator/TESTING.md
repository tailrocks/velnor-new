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
- No test starts a program found through `PATH` or touches the network. `.config/nextest.toml` wraps every test binary, when nextest lists it and when it runs it, in `env PATH=/nonexistent`, and `no_programs` fails if `git`, `cargo` or `sh` can still be found through `PATH`. A program named by an absolute path is not blocked. The `io` module is tested by the real runs.
- One behavior is tested at one layer.
- A test that runs longer than 2 s fails the suite. `.config/nextest.toml` sets that limit on `profile.default`, and `profile.ci` inherits it. The same file gives 10 s, on both profiles, only to `release_cargo_include::a_cargo_config_include_is_refused_in_every_form_in_a_real_repository`, `release_cargo_include::a_cargo_config_key_off_the_harmless_list_is_refused_in_a_real_repository`, `release_cargo_include::a_cargo_config_of_harmless_keys_is_accepted_in_a_real_repository`, `release_symlink::a_committed_cargo_symlink_hiding_a_credential_provider_is_refused_in_a_real_repository` and `release_nested_toolchain::a_root_toolchain_taken_from_a_path_is_refused_in_a_real_repository`. Other tests that run real git in a temporary repository, including `the_binary_prints_the_golden_ruleset_for_a_repository_that_holds_the_sample`, keep the 2 s timeout.

## Samples

A sample is a folder `samples/<name>/` with `view/` (the files the generator reads), `manifests.txt` (the tracked `Cargo.toml` and `Cargo.lock` paths), `symlinks.txt` (the tracked symbolic links, which may be empty but must be a regular file), `cargo-metadata/<root>.json` (recorded output) and `expected/` (the files the generator must write). A sample without `symlinks.txt` is rejected, and a sample that records a symbolic link where the generator reads a file is rejected too, because a live checkout with that link is refused.

Record a sample from a real checkout. Never type one by hand.

```sh
cargo run -p velnor-gen -- --root . record-sample <checkout> <name>
```

Add one sample for each behavior a change introduces, not one for each case. After a change of the output, rewrite `expected/` and review its diff in the pull request, because that diff is the review of what users get.

```sh
UPDATE_GOLDENS=1 cargo nextest run
```

The check `generator-samples` lints every `expected/` workflow with `jactionlint`.

The golden tests read the samples from `VELNOR_GOLDEN_SAMPLES_ROOT` when it is set, so a test can run the golden binary against a temporary copy of a sample.
