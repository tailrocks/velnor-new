# CI analysis dependency qualification

Qualified on 2026-10-03: reporting-only PyYAML **6.0.3**, MIT license,
Python >=3.8. The generator and generated workflows do not depend on Python or
PyYAML. [Requirements](../../scripts/ci-performance-analysis-requirements.txt)
pin the exact version and all 72 wheel SHA-256 hashes published in the official
[version metadata](https://pypi.org/pypi/PyYAML/6.0.3/json). No source distribution
is authorized; installation requires a published compatible wheel.

## Source identity

The official [6.0.3 tag API](https://api.github.com/repos/yaml/pyyaml/git/ref/tags/6.0.3)
resolved directly to commit `49790e73684bebad1df05ef8d828fa12f685bffb`.
Its [version declaration](https://github.com/yaml/pyyaml/blob/49790e73684bebad1df05ef8d828fa12f685bffb/lib/yaml/__init__.py)
is `6.0.3`; the commit's
[license](https://github.com/yaml/pyyaml/blob/49790e73684bebad1df05ef8d828fa12f685bffb/LICENSE)
is MIT. The tag and commit license bytes matched. The actual wheel metadata
independently reports PyYAML 6.0.3, MIT, Python >=3.8 and no runtime dependencies.
Official publication plus these identities does not establish reproducible
wheel builds or an independently verified build attestation.

Raw responses and the downloaded wheel remain mode-0600 in an external
mode-0700 private directory, recorded in `source-evidence.json`. Evidence root:
`/Users/donbeave/.codex-chainargos2/private/ci-performance/analysis-dependencies/pyyaml-6.0.3`.

| Raw evidence | Source | SHA-256 |
| --- | --- | --- |
| `pypi-version.json` | Official version metadata linked above | `c3f35597bc2f08cc990c2a5fe57bef6687b3a3d7c61d8b0ba4cc067777eb1def` |
| `source-tag.json` | Official tag API linked above | `b06de96b269fe8e6e1778bf4c363b26c5a932dc8c358092d771e6bec2bad1a4e` |
| `source-init.py` | Commit version declaration linked above | `b19dfcc333d6a75dfd73073901164507252f271b41d3b5f7d85510033a0547a7` |
| `source-commit-LICENSE` | Commit license linked above | `8d3928f9dc4490fd635707cb88eb26bd764102a7282954307d3e5167a577e8a4` |
| `pyyaml-6.0.3-cp39-cp39-macosx_11_0_arm64.whl` | [Official wheel](https://files.pythonhosted.org/packages/ae/92/861f152ce87c452b11b9d0977952259aa7df792d71c1053365cc7b09cc08/pyyaml-6.0.3-cp39-cp39-macosx_11_0_arm64.whl) | `c3355370a2c156cffb25e876646f149d5d68f5e0a3ce86a5084dd0b64a994917` |

## Isolated execution

Create the environment outside every checkout; install from the hashed wheel
allowlist without modifying the global interpreter:

```sh
rtk proxy /usr/bin/python3 -m venv /absolute/private/analysis-venv
rtk proxy /absolute/private/analysis-venv/bin/python -m pip install \
  --require-hashes --only-binary=:all: \
  -r scripts/ci-performance-analysis-requirements.txt
rtk proxy /absolute/private/analysis-venv/bin/python \
  scripts/test_ci_performance_analysis.py
rtk proxy /absolute/private/analysis-venv/bin/python \
  scripts/analyze-ci-performance.py /absolute/private/evidence/rRUN-aATTEMPT
```

For the verified downloaded wheel, installation used `--no-index` and
`--find-links` pointing to the private evidence root in addition to
`--require-hashes --only-binary=:all:`. The observed host was CPython 3.9.6,
Darwin arm64; the compatible wheel was 174,319 bytes and its downloaded SHA-256
matched official metadata exactly. Hash verification covers the downloaded
artifact; no global package installation or source build occurred.

The earlier ambient-interpreter baseline analysis was preliminary. The isolated
environment passed all **15 offline regression tests** and recomputed run
`37012391691`, attempt 1, from the existing private evidence and exact source API:
completion path **210 s**, longest dependency job walls **203 s**, executed runner
wall sum **708 s**. Workflow blob and SHA-256 remained those recorded in the
[hosted protocol](ci-performance-hosted-protocol.md). This qualifies the reporting
dependency and bounded timeline calculation, not hosted T01–T26 performance.

After terminal-metadata admission was added, the same isolated environment passed
the expanded **23-test** suite and reran that failed baseline successfully. Private
`analysis-dependencies/terminal-admission-proof/proof.json` records exact analyzer,
test, requirements and verified wheel input SHA-256 values, command exit codes
and transcript digests. The earlier 15-test count describes the initial dependency
qualification; this expansion adds admission proof, not performance qualification.

The subsequent executed-workflow revision fix restricts source resolution to
authenticated push events, binds summary/run events and rejects other events
before source lookup. The isolated **30-test** suite includes direct source
authority rejection and an actual-main PR feature/merge fixture with identical
job names and different dependency graphs. A job-level reusable workflow is also
rejected before timeline publication despite matching API job names; ordinary
step actions remain admissible. The push baseline still reproduces
210/203/708 seconds. Private
`analysis-dependencies/event-source-proof/reusable-job-fix/proof.json`
records current input and transcript digests. Historical PR calculations remain
unqualified; no mutable merge ref or inferred merge SHA is accepted.
