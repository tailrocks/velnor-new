# Cache families

Status: proposed; the family specs below are implemented in
`workflow::cache_family` (typed, pinned by test). Which families the generator
emits is stated per row. Split from the [cache contract](cache-contract.md)
section 1, which keeps the key grammar, trust model and doctor rules; the
[cache decisions](cache-decisions.md) hold the owner-adopted rules (budget,
save-on-producer-success, pruning).

Every family key is
`velnor-v<schema>-<layer>-<trust>-<compat>-<snapshot>` (`compat` and `snapshot`
are `s256-<64 hex>` digests). `trust` is `trusted` (protected default-branch
push), `pr` (same-repository pull request, PR-scoped writes only) or `none`
(forks, merge groups, dispatches: never writes). A family changes its paths or
digest inputs only with a schema bump.

| Family (schema) | Paths | Compat hashes | Snapshot | Writers | Generated |
|---|---|---|---|---|---|
| `sources` (2) | `$RUNNER_TEMP/velnor/cargo/{registry/index,registry/cache,git/db}` | family, schema, paths, `uname -s`/`-m`, `CARGO_HOME`; never a toolchain or tool version | sorted `<root>\t<sha256 of Cargo.lock>` lines | the Plan job, `trusted` or `pr` | yes |
| `tofu-providers` (2) | `$RUNNER_TEMP/velnor/tofu-cache/{root}` | as `sources`, plus `tofu version` | the root's lock file | the root's own job, `trusted` only | yes |
| `bun-install` (1) | `$RUNNER_TEMP/velnor/bun-cache` (`BUN_INSTALL_CACHE_DIR`) | family, schema, `uname -sm`, resolved Bun version | sha256 of the root's `bun.lock` | the install job on a protected push (`trusted`), plus the PR scope as for `sources` | contract only |
| `playwright-browsers` (1) | `$RUNNER_TEMP/velnor/ms-playwright` | family, schema, `uname -sm`, Playwright version from `bun.lock` | sha256 of the `directory:sha256` pins of the member's `.velnor/playwright.lock` archives | `trusted` only | contract only |
| `dbt-packages` (1) | `$RUNNER_TEMP/velnor/dbt-packages/{root}` (one directory per dbt project) | family, schema, `uname -sm`, resolved `dbt-fusion` version | sha256 of the project's `package-lock.yml` | the project's own job, `trusted` only | contract only |
| `python-venv` (1) | `$RUNNER_TEMP/velnor/uv-cache` (`UV_CACHE_DIR`) | family, schema, `uname -sm`, resolved `uv` and `python` versions | sha256 of the hash-pinned requirements file | the Ansible job, `trusted` only | contract only |
| `ansible-collections` (1) | `$RUNNER_TEMP/velnor/ansible-collections` (verified tarballs) | family, schema, `uname -sm` | sorted `name\tversion\tsha256` lines of `[[stacks.ansible.collections]]` | the Ansible job, `trusted` only | contract only |
| `mbx-bundle` (1) | `$RUNNER_TEMP/velnor/mbx-bundle` (the directory `mbx cache export --format directory` writes) | family, schema, path, `unit` (`crate` or `suite`), MBX version, Rust toolchain, workspace root, `uname -sm`, ceiling | sha256 of the workspace `Cargo.lock` | crate and suite jobs, `trusted` only; computed in the job (`Compute MBX bundle key`), not in the Plan job | yes |

`node_modules` is never cached: the isolated linker rebuilds it from the Bun
cache in seconds, and a cached `node_modules` would carry lifecycle-script
output across trust scopes.

## Bun families

- `bun-install` holds Bun's content-addressed package cache. A restore from a
  different `bun.lock` of the same compatibility is valid (the cache is keyed by
  package content), so restore keys may drop the snapshot within the same
  `trusted` scope, like `sources`. Both install phases of the registry flow
  share the directory; the lifecycle phase relies on it being warm
  ([Bun contract](bun-contract.md)).
- `playwright-browsers` holds the directories the
  `playwright-browsers-install-v1` step creates. The step trusts a restored
  directory only when its `velnor-sha256` file equals the pin, so a poisoned or
  stale entry is replaced, never used. Only `trusted` runs write it: a pull
  request must not be able to place bytes that later trusted runs reuse.
- Neither family archives credentials: the registry phase writes its token into
  a Velnor-written config under `$RUNNER_TEMP/velnor/bun/`, which is not an
  archive path.

## dbt and Ansible families

- `dbt-packages` holds the packages `dbt deps` installs for one project. Entries
  are replaced, never trusted: the job runs `dbt deps` against the committed
  `package-lock.yml` on every run, so a restored directory only saves the
  network round trip and a poisoned entry is overwritten.
- `python-venv` holds uv's wheel cache, not the virtual environment: a venv
  carries absolute paths and is cheap to rebuild with
  `uv pip install --require-hashes --no-deps` from a warm cache. Every wheel is
  re-verified against the requirements hashes at install time, so the cache
  cannot widen trust.
- `ansible-collections` holds the collection tarballs Velnor downloaded. Each is
  re-verified against the declared SHA-256 before the offline install, so a
  restored tarball that does not match is discarded.
- Only `trusted` runs write these families (a pull request must not place bytes
  that later trusted runs reuse), and none archives a credential or an
  inventory/vault file.

## MBX bundle family

- The bundle is opaque. Velnor never lists, parses or rewrites it; the only
  producer is `mbx cache export --group velnor --format directory` and the only
  consumer is `mbx cache import`, which moves the objects into the store and
  removes the restored directory.
- Compat does not name a job: crate jobs of one workspace share the entry
  (the first trusted writer wins; GitHub entries are immutable), and the key
  changes only when the lock, toolchain, MBX version or platform changes. The
  `unit` keeps crate and suite bundles apart.
- A restore may fall back to any snapshot of the same compat (the newest
  trusted entry), which `mbx cache import` then reuses object by object.
- The size check reads the on-disk bytes of the exported directory
  (`du -sk`), a conservative upper bound of the compressed entry. Over the
  ceiling the export is deleted and the save refused (`over_ceiling`).
- The bundle save has its own three-minute optional-save budget; the other
  families keep one minute.

## Size ceilings and the budget rule

Each spec carries `ceiling_bytes` (largest stored entry; a save that would
exceed it is refused and reported as `over_ceiling`) and `retained_entries`
(newest entries kept per unit; older exact keys are pruned per family,
[decision 4](cache-decisions.md)). `worst_case_bytes(units)` multiplies them.
A test sums the reference repository (ChainArgos: four Bun roots, two OpenTofu
roots, one Playwright version, one dbt project, one requirements file, one collection set, one Cargo workspace bundle, plus a 1.5 GiB allowance for the tools layer)
and fails when it exceeds 85 % of the 10 GiB quota.

| Family | Ceiling per entry | Retained per unit | Unit | Reference units | Worst case | Measured |
|---|---:|---:|---|---:|---:|---|
| `sources` | 128 MiB | 3 | repository | 1 | 384 MiB | 16.8 MiB stored ([measurements](../implemented/cache-measurements.md)) |
| `tofu-providers` | 256 MiB | 3 | OpenTofu root | 2 | 1.5 GiB | not measured; ceiling is a placeholder to revise with data |
| `bun-install` | 256 MiB | 2 | Bun root | 4 | 2 GiB | 30.3 MB gzip (115 MB raw) for a 61-package frontend (Bun 1.4.2) |
| `dbt-packages` | 64 MiB | 2 | dbt project | 1 | 128 MiB | not measured; ceiling is a placeholder to revise with data |
| `python-venv` | 384 MiB | 2 | requirements file | 1 | 768 MiB | not measured; ceiling is a placeholder to revise with data |
| `ansible-collections` | 128 MiB | 2 | collection set | 1 | 256 MiB | not measured; ceiling is a placeholder to revise with data |
| `playwright-browsers` | 512 MiB | 2 | Playwright version | 1 | 1 GiB | 328 MB of Chromium archives (Chromium, headless shell, ffmpeg; Playwright 1.64.1) |
| `mbx-bundle` | 512 MiB | 1 | Cargo workspace | 1 | 512 MiB | not measured; ceiling is a placeholder to revise with data (a superseded entry is pruned by decision 4) |

The reference total with the tools allowance is about 8 GiB against the
8.5 GiB budget (85 % of 10 GiB). A real Bun root that stores more than 256 MiB is
refused at save time and reported, never silently truncated; the owner raises
that ceiling with a measurement.

## Remaining

The Plan `Compute cache keys` step emits keys for `sources` and
`tofu-providers` only. Generating the `bun-install`, `playwright-browsers`,
`dbt-packages`, `python-venv` and `ansible-collections`
restore and save steps (per-root keys as Plan outputs, saves gated on the
producer step and `cache_trust`, the pruning step) follows the `tofu-providers`
per-root pattern and is not done yet; until then the cache doctor rejects any
step on these families, so a hand-written step cannot bypass the gates.
