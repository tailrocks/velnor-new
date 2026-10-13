# CI performance evidence recapture: 2026-10-05

Status: **source-bound historical runs and a current scope snapshot only**. This
recapture closes no C01–C09 gate or T01–T26 experiment. All 47 scoped repository
rows remain `INCOMPLETE`; all 47 controlled cold/warm/third experiments remain
`NOT_PERFORMED`.

## Collection identity

Captured at `2026-10-05T00:39:57.030406Z`. The combined mode-0600 receipt is
retained outside Git at
`/root/.local/share/velnor/ci-performance/pr12-2026-10-05/evidence-recapture-20261005.json`,
SHA-256
`8d08d624b56f84f6476f109a4aaa1025051ad5291257411ba1ddfa53ac6ae17b` (51,521
bytes). It contains run, attempt, job, workflow, artifact-index, archive-digest,
tool-source, analyzer-test, and Exact47 snapshot fields. Raw receipt and private
repository head details are not copied into this public record.

The read-only collector was `scripts/collect-ci-performance.py` (source SHA-256
`4e3e8afa2a6edbfbf24612d6e093232171059cc0bf99a39958f530d74db308cc`). Analysis
used `scripts/analyze-ci-performance.py` (SHA-256
`d74f09f48c21c3d980b456acb6eb341bad35a3c0bf0f28c77bef20760bbd0cfe`) with the
exact-hash requirements file (SHA-256
`641c90b7a2740c4f37f6a7a6ecf49e56b4679bf1f778abf083f6fa3dc1d8a263`), PyYAML
6.0.3, and uv 0.12.23. The analyzer test command passed 30/30 tests:

```sh
/tmp/velnor-pr12-analyzer-uv-venv/bin/python -m unittest scripts/test_ci_performance_analysis.py
```

This verifies the pinned analyzer behavior only. It is not a generator gate,
hosted qualification, or performance result.

## Historical source-bound attempts

| Repository | Run / attempt | Immutable workflow source | Result and derived timing |
|---|---|---|---|
| `jackin-project/jackin` | [37122829759 / 1](https://github.com/jackin-project/jackin/actions/runs/37122829759) | Commit `0aa821a088e1bacf3d4d85a4c9faaa67faa85132`; `.github/workflows/ci.yml` Git blob `3f05f66441de835b8d066230fa564e30ffdc16af`; raw SHA-256 `e047459428877a9b7cb8ab01cccc884ed290b48b993c4421142e57191f391327` | 31/31 jobs completed successfully; run-wide artifact listing has 30 items. Workflow dependency-path calculation: 865 s; summed runner wall: 5,704 s. The separately recorded all-job envelope is 875 s. |
| `tailrocks/velnor-new` | [37157003048 / 1](https://github.com/tailrocks/velnor-new/actions/runs/37157003048) | Commit `9a249e9ab99a019ab7ba0e6f835dee406827db20`; `.github/workflows/ci.yml` Git blob `c73ae521905a29b75784ab3c89c4a95ef914dbef`; raw SHA-256 `879a2abc9626f3b3b5d45bd26f8b09b21fef0adc226b4f32e106cdfde9049160` | 20/20 jobs completed: 17 success, 2 failure, 1 skipped. Run-wide artifact listing has 16 items. |
| `tailrocks/velnor-new` | [37157003048 / 2](https://github.com/tailrocks/velnor-new/actions/runs/37157003048) | Same exact commit and workflow as attempt 1 | 20/20 jobs completed successfully. Workflow dependency-path calculation: 533 s; summed runner wall: 1,748 s. The separately recorded all-job envelope is 546 s. |

These are ordinary historical CI attempts, not a controlled experiment sequence.
The two generator attempts use the same source, but attempt 1 failed and they
were not isolated cold, warm, and third runs with validated identical workload
domains. Neither run establishes compiler work avoided, payload equivalence,
cache persistence, or a performance improvement. Queue time, runner
provisioning, compiler process time, link time, tool-download bytes, and other
unsupported measurements remain unknown; no unknown is treated as zero.

The generator artifact endpoint is run-wide. Attempt 1's response includes
artifacts whose names encode attempt 2, so those do not belong to attempt 1.
Artifacts whose names omit an attempt marker remain bound to the run/source,
not to an individual attempt. API digest listings are not equivalent to
independently hashing every artifact archive.

## Independently streamed archive digests

Six selected artifact ZIP response bodies were streamed and SHA-256 hashed. Each
body digest matched the GitHub Actions artifact API digest:

| Repository | Artifact ID / name | Matching SHA-256 |
|---|---|---|
| Jackin | `11273717278` / `velnor-plan-r37122829759-a1` | `9a6c5fa6d68b3210ce6c97d4336bb102857e10538eadef89a9926f53aef9977a` |
| Jackin | `11274402613` / `velnor-final-r37122829759-a1` | `8e2863359200d66158ae70a8599513f17f48a105ef4507305ec9765624dde9ba` |
| Jackin | `11273968446` / baseline artifact | `e06f0ff986f10dd53c931decd268c03b21a0e1dbe36a9c3cb63a58348a5f9796` |
| Generator | `11287116356` / `velnor-plan-r37157003048-a2` | `b188673ce0947895a21729ddeba5781595794dc363c6f654ef0fd5bb0803b32d` |
| Generator | `11287241826` / `velnor-final-r37157003048-a2` | `0adb7da50ac4fcbecd4aed4e33e2b7fd065b6a831c0c3e8b0d31b8dfb56b15e3` |
| Generator | `11287207034` / baseline artifact | `b6fc02398ff4fe8f640f664ad5106ac54ba84883abe9ae0152a8e5f9d713370a` |

For generator attempt 1, the final and plan artifacts named with `a2` are not
attempt-1 evidence. Baseline artifact names do not encode an attempt. The other
artifact archive bodies were not downloaded or hashed in this recapture.

## Current Exact47 branch snapshot

The read-only GitHub API snapshot queried exactly the 47 repositories in the
performance scope against their default branches. At capture, all 47 were
accessible, all default branches were `main`, and four repositories were
private. The private head list remains in the private receipt. Compared with the
2026-10-04 refresh, exactly two public heads had advanced:

| Repository | Previous head | Captured default head |
|---|---|---|
| `tailrocks/velnor-new` | `5a946c33cf005777feab2bc91fa4aa8e01dd58f4` | `ad73ae9f0500ddd02d64aad142bbecb2122c0617` |
| `tailrocks/velnor` | `3f6633252963efef0d71244aadae36516a11601e` | `72f0fb14ec8f477fce8a505a624b92837acab85d` |

This refresh updates source-location awareness only. It does not re-audit all
47 repositories' obligations, establish workflow adoption, or change any
performance status. The ledger's frozen W0 SHAs are historical audit identities,
not assertions about these live heads.

## Qualification workflow availability

The captured `tailrocks/velnor-new` default head was `ad73ae9f0500ddd02d64aad142bbecb2122c0617`.
Its `qualification.yml` dispatch is the G4 Scale Set and feature smoke; it does
not run a full Exact47 performance qualification. Regular CI has no manual
dispatch path, and cache-save steps are restricted to trusted push events. The
PR12 `foundation-qualification.yml` candidate is not present on that default
head and covers only native Foundation positives. No supported default-branch
workflow path was available at capture for a full controlled cold/warm/third
sequence. This is an implementation gap. Do not dispatch the G4 smoke or infer
performance qualification from it.

The new workflow must be reviewed and integrated before any full qualification
dispatch. It must bind exact source and candidate identities, keep a dedicated
isolated cache namespace, retain the complete required workload, and provide
separate controlled cold/warm/third observations. This evidence record does
not define a substitute switch or grant permission to bypass generator
obligations.
