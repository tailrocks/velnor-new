# bun-workspace-suites fixture
Intent: the `bun-workspace` repository opting into suites (a sanitized copy
of the ChainArgos platform configuration): `ci` checks stay in the member
job, vitest runs in a `unit` suite and Playwright in a `browser` suite.
Expected outcome: `input/` generates `expected/.github/workflows/ci.yml`
byte for byte (`impl_bun_golden.rs`); it changes only when a suite feature
changes on purpose (`VELNOR_UPDATE_GOLDENS=1`).
