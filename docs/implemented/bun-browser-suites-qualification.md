# Bun browser suites: qualification

Feature: [Bun browser suites](../proposed/bun-browser-suites.md) (hard decision 6
amendment). Design checks Q1-Q7. Pinned tools: Bun 1.4.3, Playwright 1.64.1,
`actions/upload-artifact@cf430e03`, `actions/download-artifact@9000827c`,
`actions/cache@55cc8345`, runner `ubuntu-26.04`.

## Method

Generated workflows acquire a released `velnor-actions` (the release manifest
pins `tailrocks/velnor-new` release URLs), so a fixture repository cannot run a
generated suite workflow before the release that carries it. Q1-Q5 therefore
ran in two ways:

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

Q6 and Q7 exercise the generated plan, the assets job and Required; they run
against the release that ships these changes (see "Pending").

## Results

| Check | Result | Evidence |
|---|---|---|
| Q1 `bun run <script> --shard=i/n --project=p` reaches Playwright, directly and through a `bun x.ts` wrapper | pass | local blobs: metadata `shard {current, total}` and one `onProject` per leg; live run 39757712469, every leg logs `velnor: blob verified shard=i/2 projects=p` through the wrapper |
| Blob verifier against real 1.64.1 blobs | pass | sharded, unsharded and wrapper blobs verify; an unsharded blob for a sharded leg, a foreign project, a wrong shard and a blob without metadata fail with `playwright_blob_shard_mismatch`, `playwright_blob_project_mismatch`, `playwright_blob_missing` |
| Q2 legs green, merged report holds every test exactly once | pass | live run 39757712469: 4 legs green, report job `merged tests=40 unique=40`, 40 expected; local merge of 4 blobs: 40 of 40 |
| Q3 single writer; zero CDN downloads on the second run; chromium + webkit stored size | pass | run 39757712469: only leg 1 ran `Save playwright-browsers cache` (one entry, 369.09 MiB); run 39758262409: all 4 legs restored it and made zero downloads; `tar \| zstd` of chromium, chromium headless shell, ffmpeg and webkit: 386,984,701 bytes (369 MiB), under the 512 MiB ceiling |
| Q4 one failing leg turns the run red; "Re-run failed jobs" re-runs only that leg; blob overwrite; report re-merges | pass | run 39758262409 attempt 1: leg 3 failed, the matrix aggregate (what Required needs) failed, the report still merged (40 tests, 1 unexpected); attempt 2 re-ran only leg 3 and the report; leg 3's blob was replaced (4449 to 3041 bytes) and the other legs' attempt-1 blobs were downloadable; the re-merged report shows 40 tests, 0 unexpected |
| Q5 leg timing and setup share | fixture only | warm leg about 30-45 s wall clock, of which the tests take under 1 s; a cold leg adds the 369 MiB browser download. Platform and documentation-portal timings come with their adoption PRs |
| Q6 fork pull request: Required fails closed with pinned assets, no cache saved | pending | needs the release (assets job) |
| Q7 a `not_affected` member's suites and legs are skipped and Required is green | pending | needs the release (plan dispositions); covered by `impl_bun_select.rs` and the Required fold tests |

## Pending

After the release that carries these changes, regenerate a fixture with the
`browser` suite of `fixtures/bun-workspace-suites` plus pinned release assets
and run: a fork pull request (Q6: the assets job's mint fails, the suites are
skipped, `Required` is red with `planned_job_skipped`, no cache entry is
written), and a pull request that changes only another member (Q7: the suite
job and its legs are skipped, `Required` is green).
