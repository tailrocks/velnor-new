# bun-workspace fixture
Intent: a Bun root workspace (`bun = "1.4.3"`, two members, Chromium on
`@fixture/web`, a hash-pinned `.velnor/playwright.lock`) that declares no
suite.
Expected outcome: `input/` generates `expected/.github/workflows/ci.yml`
byte for byte (`impl_bun_golden.rs`). Later Bun features must leave it
unchanged; refresh only deliberately with `VELNOR_UPDATE_GOLDENS=1`.
