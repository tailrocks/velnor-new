# Testing the generator

| Layer | Proves | Where |
|---|---|---|
| Rule tests | One function: this input gives this output | Next to the code, in each crate |
| Golden tests | A whole sample repository gives exactly the expected files | `crates/velnor-gen/tests/it/golden.rs` |
| Helper tests | `velnor-ci` commands against temporary folders and recorded API responses | `crates/velnor-ci/tests/it/` |
| Real runs | The generated workflow works on GitHub | `velnor-lab` nightly and this repository's CI |

## Rules

- Assert what a user of the generator can observe: the written files, the selection, an error's type and path. Never the wording of a message, the length of a list, or a private structure.
- A test name states the behavior, for example `docs_only_change_selects_nothing`.
- A rule test is a table of inputs and outputs, not one test per case.
- No test starts a program found through `PATH` or touches the network. `.config/nextest.toml` wraps every test binary, when nextest lists it and when it runs it, in `env PATH=/nonexistent`, and `no_programs` fails if `git`, `cargo` or `sh` can still be found through `PATH`. A program named by an absolute path is not blocked. The `io` module is tested by the real runs.
- One behavior is tested at one layer.
- Timeouts are set in `.config/nextest.toml` (at the repository root), not in this file. Overrides are read in file order, and the first one that matches a test sets its limit. There are three tiers, 2 s, 10 s and 30 s:
  - 2 s is the catch-all on `profile.default` for the velnor-ci, velnor-gen, velnor-model, velnor-release, velnor-render and velnor-rust packages. A test that runs longer fails the suite, unless a named 10 s or 30 s override matches it first. `profile.ci` has no 2 s line.
  - 10 s applies to 11 named velnor-gen tests, on both `profile.default` and `profile.ci`: three `release_cargo_include` tests, `release_symlink::a_committed_cargo_symlink_hiding_a_credential_provider_is_refused_in_a_real_repository`, `release_ruleset_lock_version::the_ruleset_command_refuses_a_lock_version_of_a_released_member`, `release_nested_toolchain::a_root_toolchain_taken_from_a_path_is_refused_in_a_real_repository`, four `golden_env` tests and `helper_tool_paths::setup_records_non_default_tools_and_rendered_calls_use_them`. The 2 s catch-all does not cover these. Other tests in those modules keep 2 s.
  - 30 s applies to named velnor-ci tests. On `profile.default` these are the 13 `io_*` test modules (for example `io_restore_cargo` and `io_scope_cache`), `crates_mirror`, `audit::bounds::`, two `audit::migration` tests and a few more. On `profile.ci` they are `crates_mirror`, `audit::bounds::`, two `audit::migration` tests, `plan_reads_each_roots_sources_to_decide_the_doctest_and_documentation_steps` and a few named tests; the `io_*` modules have no override there. On `profile.ci`, other velnor-ci tests have no limit, because that profile has no 2 s line.
  - A test that builds a temporary repository and runs real git, such as `ruleset::the_binary_prints_the_golden_ruleset_for_a_repository_that_holds_the_sample`, is on none of the named lists and keeps the 2 s catch-all.
  - `profile.ci` repeats the 10 s and 30 s overrides. It has no 2 s catch-all line.
  - A test that builds a temporary repository and runs real git, such as `ruleset::the_binary_prints_the_golden_ruleset_for_a_repository_that_holds_the_sample`, is on none of these lists and keeps the 2 s catch-all.

## Samples

A sample is a folder `samples/<name>/` with `view/` (the files the generator reads), `manifests.txt` (the tracked `Cargo.toml` and `Cargo.lock` paths), `symlinks.txt` (the tracked symbolic links, which may be empty but must be a regular file), `cargo-metadata/<root>.json` (recorded output) and `expected/` (the files the generator must write). A sample without `symlinks.txt` is rejected, and a sample that records a symbolic link where the generator reads a file is rejected too, because a live checkout with that link is refused. A symbolic link at a Rust source path is the exception: the generator does not read it, and the crate that holds it is selected by any change, because the file it points at is unknown.

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
