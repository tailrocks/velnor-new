# Mise platform binary pins

Verified 2026-10-03 for Mise `2026.10.0`. The `jdx/mise-action` `sha256`
input identifies the installed binary, not the compressed archive.
`MiseSetup::for_target` chooses the binary digest from the explicit job platform.
Normal plan/final jobs retain the global Linux runner; named checks may use
validated native macOS or ephemeral self-hosted runner profiles.

| Target | Official archive | Archive SHA-256 | Extracted `mise/bin/mise` SHA-256 |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `mise-v2026.10.0-linux-x64.tar.gz` | `6ae3d2bda39cca86713501317edf623b59b75cd8afa2e31c20b1ee6500b4b739` | `57ced973f968b8fbab07aa8e32bd7077d4a357e200a22356d98963c723c6de0a` |
| `aarch64-apple-darwin` | `mise-v2026.10.0-macos-arm64.tar.gz` | `e6a966e44f871403df905d50019ca6f7b84624ddfe1d5c095f5bc3f18709259e` | `8d2007efdae0c2b64e3955257533e6ec17197bc2fdcbc5dd8f6847f92881deea` |
| `x86_64-apple-darwin` | `mise-v2026.10.0-macos-x64.tar.gz` | `6bb6c2239b8990c81350d4f9e46bfffd6cfaf48f967b2f98963ce24b944400f0` | `815eb7872e453dcd30ed5e2e478978d2947e34106b6cd4f9cc4d4e51f7761332` |

Qualification downloaded the official archives, compared each archive digest
against the release [SHASUMS256.txt](https://github.com/jdx/mise/releases/download/v2026.10.0/SHASUMS256.txt),
and hashed each extracted binary independently. Only the compatible macOS
ARM64 binary was executed; `--version` and `--help` exited successfully.

Archives: [Linux x86-64](https://github.com/jdx/mise/releases/download/v2026.10.0/mise-v2026.10.0-linux-x64.tar.gz),
[macOS ARM64](https://github.com/jdx/mise/releases/download/v2026.10.0/mise-v2026.10.0-macos-arm64.tar.gz),
[macOS x86-64](https://github.com/jdx/mise/releases/download/v2026.10.0/mise-v2026.10.0-macos-x64.tar.gz).

Recorded verification artifacts reside under
`/Users/donbeave/.local/state/jackin-verification/20261002T212102Z/upstream-platform/`:
`mise-2026.10.0-platform-qualification.json` (SHA-256
`f7d289125e094176563f5c9be053e52b20fe12984b1b3219fe3f8ac33d37090d`)
and `mise-2026.10.0-task-qualification.json` (16/16 checks passed).
Task qualification covers exclusive configuration, poisoned ambient
configuration exclusion, native environment, dependency tasks, and repeated
nested execution after freshness metadata removal. These are bounded local
fixtures; CI platform execution is verified separately.

Strict setup validates an existing qualified setup against the exact selected
binary pin, action ref, and target/tool cache key. An otherwise well-shaped
Linux setup in a macOS check fails with `setup_mise_pin_mismatch`.
Helper acquisition uses the selected release target; macOS verifies downloads
with native `shasum -a 256 -c -`. Unsupported helper targets fail generation.

Preseed builds produce one Linux helper. A check targeting another platform
requires a bootstrap release lock containing that platform's helper asset;
generation fails with `mixed_platform_preseed_requires_release_lock` instead
of staging the Linux helper on macOS.
