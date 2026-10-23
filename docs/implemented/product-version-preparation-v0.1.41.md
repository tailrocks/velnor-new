# Product version preparation for v0.1.41

This prepares the v0.1.41 product release from main
`1add37e2d3c4aaceefe9e934ece92e274bec00a9` (#355). The immutable v0.1.40
release was published from `3118a34c13481a4a303af7ad51464a614d42962b` and
stays unchanged.

v0.1.41 ships everything merged since v0.1.40:

- Immutable preview source selection on a main branch protected only by
  rulesets (#355, fixes #354): the classic protection route's exact
  `404 Branch not protected` answer means "no classic source", and the
  `preview-publish`/`tap-update` environments accept the single
  `branch_policy` rule GitHub reports for a custom deployment branch policy.
  Every other answer still fails closed.
- Empty CI run inventory retries and a cold planning budget (#353).
- The owner mise and hk standard (#348).
- Playwright locks the browsers a member's suites declare (#356) and installs
  WebKit and Firefox system libraries on hosted runners (#358).

The root workspace version and exact internal path requirements advance from
0.1.40 to 0.1.41. Cargo controls the lock update: only the fourteen workspace
package versions change; external package records, checksums and dependency
graphs remain unchanged. The generated `.github` tree, the parity fixtures and
the OpenTofu goldens were regenerated with `velnor-actions generate`,
`VELNOR_UPDATE_GOLDENS=1` and `scripts/capture-opentofu-goldens.sh capture`
(then `check`: all goldens match). The only differences are the version stamp
and the hashes derived from it. The independently versioned runner workspace,
tool pins and historical release manifests are unchanged.

This is version preparation, not publication. Exact-source hosted CI,
protected release publication, accepted manifests and public verification
remain separate requirements.
