# Signed attestation fixtures

The v0.1.5 manifest, bundles, and Linux binary came from the [v0.1.5 release](https://github.com/tailrocks/velnor-new/releases/tag/untagged-6899c9b4aa4e941dadba). `binary.gz` contains the original `velnor-actions-0.1.5-x86_64-unknown-linux-gnu` bytes, whose SHA-256 is `040c647a575684b84f28588862bad78876faf0224d8e5538c7a680d7d528f6b3`. Its compressed SHA-256 is `3a198c97f6b28c58c7ce20c3f99d47da986444ecffc1f70f5acab1b63cb73ad1`.

The `historical` manifest and bundle came from the [v0.1.73 release](https://github.com/tailrocks/velnor-new/releases/tag/v0.1.73). The trusted root was obtained with `gh attestation trusted-root` and has SHA-256 `65ca537f6ed8a47fd0e560c421baa1f6c1efb8b25fc200d8c5c02c0e92eb2b9c`. The tests use these offline assets to verify signatures; GitHub API responses in tests are simulated and do not establish current repository ownership.
