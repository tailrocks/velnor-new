# Family tag for `[[binaries]]` releases (#1290, PR 2)

## What it does

`family_tag` in `.velnor/release.toml` (opt-in, see `generator/CONFIG.md`) gives all `[[binaries]]` of a release one tag, for example `v0.1.80`, bound to the verified source commit:

- A `family` job reads the binaries' version from `cargo metadata` at the verified commit (all must agree), and runs the new `velnor-ci release family-tag`. The helper creates the tag on the verified commit, refuses unless the tag resolves to that commit itself, and opens one draft release.
- Builds check out `refs/tags/<family tag>` and refuse any commit other than the verified one. One `sums-family` and one `publish-family` job publish all assets of all binaries under the one tag.
- The generator refuses a config where a binary is still released by release-plz under its own tag, where the pattern names `{{ package }}`, or where the helper is older than `0.1.82`.
- With `family_tag` unset the output is byte-identical (all 765 existing generator tests and goldens pass unchanged).

## What now matches the old engine

| Old engine (`product-release-generator.yml`, `product-release-binary.yml`) | Now |
|---|---|
| One family tag per release, fixed to the version (`v0.1.80`) | `family_tag = "v{{ version }}"` |
| Tag created via the git refs API at the verified `source_sha` | helper POSTs the ref at the verified commit |
| `assert_tag_target`: tag must resolve to the exact source commit, else refuse | helper refuses unless tag target == verified commit; builds repeat the check |
| Release published at an earlier ancestor commit is a no-op with a warning | `build=false`, with the `ahead` comparison |
| Draft release created at the source commit, one asset set, published once | one draft by the App, one publish job |
| Build from the exact source commit | builds check out the tag and compare to the verified commit |

Proof: a throwaway config on the recorded velnor-new sample (roots `generator` and `ci-helper`, binaries `velnor-gen` and `velnor-ci`, `family_tag = "v{{ version }}"`) generated a `release.yml` with jobs gate, release-pr, tag, family, 6 build jobs, sums-family, publish-family and finalize. The old files have none of these as such. The throwaway was not committed. A committed sample, `generator/samples/release-bin-family`, covers the feature.

## Gaps that remain

- Old trigger and orchestration: `workflow_call` with `source_sha`, `workflow_authority_sha`, CI run inputs, and the `verify-release-caller` and eligibility jobs (gap E in the issue: owner decision to move to `workflow_run`).
- Old tags that are not version-based: `binary-<sha>` and `runner-<sha>` (the `velnor-host` and image families). `family_tag` only supports `{{ version }}`.
- Fixed-tag immutability checks: `immutable == true`, `gh release verify`, `verify-asset`, and the `gh attestation verify` calls (gap C and PR 1).
- Asset format (gap A), multi-binary packaging (gap B), qualify, candidate-manifest and attest-manifest jobs (gap D), macOS x64 (dropped by the owner), source preview and homebrew jobs (later PRs).
- The helper `release family-tag` lands here with its tests but needs a helper release that contains it. The generator requires `0.1.82` or later, which is an assumption to confirm when the helper is released.
- A version bump is by hand or PR: binary packages are `release = false` in release-plz, so release-plz does not bump them. The family job tags whenever the current version has no release. A re-run at the same version is a no-op.
- No real run: this is untested on GitHub; the proof is generated output and unit tests.
