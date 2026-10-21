# Product version preparation for v0.1.38

v0.1.37 was published from `2f090536`. This prepares the next generator
release, which carries the changes merged to main after that commit:

- #287: per-family pruning of superseded cache entries in a push-only workflow.
- #286: Playwright archive download falls back to Playwright's documented
  mirrors, hash-checked against the lock.
- #285: stable dbt-fusion `X.Y.Z` versions are accepted.
- #289: host-dependent tests are hermetic, and killed children are reaped under
  load.
- #292: restore, size-admission and save steps for the Bun, dbt and Ansible
  cache families.
- #288: verification jobs accept hosted jq 1.8 patch releases, and Mise is
  capped above the task root.
- #291: `private-fetch-v1` fetches Cargo git sources with the project-declared
  Rust into the owned Cargo home, and fails closed when that toolchain is not
  installed. Before this fix, every consumer whose Rust differed from the Velnor
  pin fetched its private git sources into `~/.cargo`. The offline `Fetch Cargo
  sources` step then fell back to an unauthenticated clone
  (bitcoin-processor-rust run 39591265476). The PR also adds the stable
  `.velnor/deps/verified.json` marker.

The fourteen workspace package versions and their exact local dependency
requirements advance to 0.1.38. Cargo controls the lock update: the fourteen
local package records change, and every registry record keeps its version,
checksum and dependency graph. The independently versioned runner workspace,
the tool catalog and the project tool configuration are unchanged.

Every version-bearing generated file was regenerated with the 0.1.38 binary:

- the repository `.github` tree, through `velnor-actions generate`;
- the parity fixtures, through `VELNOR_UPDATE_GOLDENS=1`;
- the OpenTofu goldens, through `scripts/capture-opentofu-goldens.sh capture`.

`capture-opentofu-goldens.sh check` reports that all goldens match and that
the dogfood tree is identical.

This is preparation, not publication. Exact-source hosted CI, the protected
`generator-release` approval, public artifact verification and consumer
adoption remain separate requirements.
