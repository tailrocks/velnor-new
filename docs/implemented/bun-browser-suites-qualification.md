# Bun browser suites: qualification

Feature: [Bun browser suites](../proposed/bun-browser-suites.md) (hard decision 6
amendment). Design checks Q1-Q7. Pinned tools: Bun 1.4.3, Playwright 1.64.1,
`actions/upload-artifact@cf430e03`, `actions/download-artifact@9000827c`,
`actions/cache@55cc8345`, runner `ubuntu-26.04`.

Status: Q1-Q4 below preserve historical fixture observations, not qualification
evidence for the current source. These checks were fixture-scope only: the live
throwaway workflow reproduced selected generated steps but did not run a
released generated workflow. The exact source and throwaway-harness commit SHAs
were not recorded, and the cited GitHub runs are no longer available. The
repaired #337 asset handoff and report path have not been qualified end to end.

## Method

Generated workflows acquire a released `velnor-actions` (the release manifest
pins `tailrocks/velnor-new` release URLs), so a fixture repository cannot run a
generated suite workflow before the release that carries it. The historical
Q1-Q5 observations ran in two ways:

- locally, with the pinned Bun and Playwright over a 2-project, 40-test
  `fullyParallel` fixture whose tests need no browser;
- live, in the throwaway private repository `tailrocks/velnor-shard-qual`
  (deleted afterwards), whose workflow reproduces the generated leg, verify,
  upload, single-writer cache and report-merge steps byte for byte where they
  touch GitHub or Playwright semantics (matrix `include` of 2 projects x 2
  shards, `fail-fast: false`, the env-only flags, the exact blob verification
  program, `overwrite: true` uploads, the `pattern` + `merge-multiple`
  download, `continue-on-error` report job). The leg script runs through a
  `bun scripts/with-node.ts playwright test` wrapper, the documentation-portal
  shape.

The throwaway repository was deleted after the runs. Neither its commit SHA nor
the exact source SHA used for the local and live observations was retained in
the record. GitHub now returns 404 for runs 39757712469 and 39758262409. These
details prevent reproducing or binding those observations to the current source;
the methods and measurements below remain useful fixture history only.

Q6 and Q7 exercise the generated plan, the assets job and Required; they run
against the release that ships these changes (see "Pending").

## Results

| Check | Result | Evidence |
|---|---|---|
| Q1 `bun run <script> --shard=i/n --project=p` reaches Playwright, directly and through a `bun x.ts` wrapper | historical fixture observation; not qualifying | local blobs showed metadata `shard {current, total}` and one `onProject` per leg; the unavailable live run 39757712469 reportedly logged `velnor: blob verified shard=i/2 projects=p` through the wrapper |
| Blob verifier against real 1.64.1 blobs | historical fixture observation; not qualifying | the recorded sharded, unsharded and wrapper blobs verified; an unsharded blob for a sharded leg, a foreign project, a wrong shard and a blob without metadata failed with `playwright_blob_shard_mismatch`, `playwright_blob_project_mismatch`, `playwright_blob_missing` |
| Q2 legs green, merged report holds every test exactly once | historical fixture observation; not qualifying | the unavailable live run 39757712469 reportedly had 4 green legs and `merged tests=40 unique=40`; a local merge of 4 fixture blobs reported 40 of 40 |
| Q3 single writer; zero CDN downloads on the second run; chromium + webkit stored size | historical fixture observation; not qualifying | recorded runs: only leg 1 saved one 369.09 MiB entry; the second run restored it on all 4 legs with zero CDN downloads. Fixture `tar \| zstd` measured chromium, headless shell, ffmpeg and webkit at 386,984,701 bytes (369 MiB), under the 512 MiB ceiling |
| Q4 failing leg, rerun and report merge | historical fixture observation; not qualifying | run 39758262409: attempt 1 had leg 3 and the matrix aggregate (Required) fail while the report merged with 1 unexpected test. Attempt 2 reran only leg 3 and the report; leg 3's blob changed from 4449 to 3041 bytes, other attempt-1 blobs remained downloadable, and the report had 40 tests, 0 unexpected |
| Q5 leg timing and setup share | fixture timing only | the recorded fixture had a warm leg of about 30-45 s, with tests under 1 s; a cold leg added the 369 MiB browser download. These measurements do not qualify product or platform timing. Platform and documentation-portal timings come with their adoption PRs |
| Q6 fork pull request: Required fails closed with pinned assets, no cache saved | pending | needs the release (assets job) |
| Q7 a `not_affected` member's suites and legs are skipped and Required is green | pending | needs the release (plan dispositions); covered by `impl_bun_select.rs` and the Required fold tests |

## Pending

Rerun Q1-Q4 against the released generated workflow before treating them as
qualification results. Record the exact source SHA, fixture/harness SHA and
run URL with the evidence. The #337 verified-asset handoff and report path also
needs an end-to-end run against the released artifact before it is qualified.

After the release that carries these changes, regenerate a fixture with the
`browser` suite of `fixtures/bun-workspace-suites` plus pinned release assets
and run: a fork pull request (Q6: the assets job's mint fails, the suites are
skipped, `Required` is red with `planned_job_skipped`, no cache entry is
written), and a pull request that changes only another member (Q7: the suite
job and its legs are skipped, `Required` is green).
