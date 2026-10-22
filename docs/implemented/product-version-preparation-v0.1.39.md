# Product version preparation for v0.1.39

This prepares the v0.1.39 product release from main `fd1cc81c8` (#334). It
advances the workspace version and its exact internal path requirements from
0.1.38 to 0.1.39 and regenerates every generated file with repository tooling.

v0.1.39 ships everything merged since v0.1.38 (`f14043d24`, #293):

- jactionlint replaces actionlint as the only workflow linter (#301, #302, #305,
  #311, #312, #313), including self-hosting at the profile default, the consumer
  rollout procedure and the retirement of zizmor, whose audits jactionlint
  carries.
- Security fixes for upgrade patches and Rust/Mise installs (#303, #304, #306
  to #309, #327, #328, #332): whole-patch validation, publication of only a
  workflow-regenerated `.github/`, per-file Rust verification also after a cache
  restore, restored Mise installs re-verified against their pinned archives,
  hermetic restore checks, and a Rust check plus workflow audit in every
  generated job.
- Bun suite sharding (#330, #331, #333, #334).
- The publish database (#294), the bounded plan job timeout (#249), the
  Bun-suite amendment of hard decision 6 (#329), and the Bun member job golden
  (#331).

Cargo controls the lock update: only the fourteen workspace package versions
change; all external package records keep their versions, checksums and graphs.
The generated `.github` tree, the parity fixtures and the OpenTofu goldens were
regenerated with `velnor-actions generate`, `VELNOR_UPDATE_GOLDENS=1` and
`scripts/capture-opentofu-goldens.sh capture`; the only differences are the
version stamp. The independently versioned runner, tool pins and historical
manifests are unchanged.

This is version preparation, not publication. Hosted exact-source CI, protected
release publication and public verification remain separate requirements.
