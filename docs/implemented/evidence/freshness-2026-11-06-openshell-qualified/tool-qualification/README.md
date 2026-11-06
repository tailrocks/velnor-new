# Local tool qualification receipts

The macOS ARM64 and BuildKit receipts bind to the same source snapshot at `55b31ce8ade5f560bcdd2db7e521f5c0baf41496`. Both source manifests contain 4,683 files and have SHA-256 `d1a10714b6416de255d0e9960a61659edce70ee910bb841b38fc0cf60a0f6b30`.

## Native macOS ARM64

`macos-arm64-host.txt`, `macos-arm64-commands.txt`, `macos-arm64-archive-and-executable-sha256.txt`, and `macos-arm64-source-manifest.json` preserve host identity, argv, verified archive and executable hashes, and the source snapshot. The prefixed version outputs and exit files bind the reported jactionlint, ShellCheck, OpenTofu, and ALint versions. The unprefixed probe outputs, exit files, `workflow-files.txt`, `archive-manifest.tsv`, and `release-digests.json` preserve the actual positive and negative behavior probes and the verified input records.

## BuildKit Linux authoring command

`buildkit-qualify-tools.command`, `.exit`, `.stdout`, and `.stderr` preserve the public `velnor-actions qualify-tools` invocation and output. `qualified-tools-authoring-candidate.json` has SHA-256 `ed391749b2259110d25cfe7c9f29e0a0387be1a9363c4445b76b108aa421e546`; it contains the measured Linux x64 jactionlint `2.5.0` and ShellCheck `0.11.0` tuples. `buildkit-host.txt` records the actual BuildKit environment. Its x86_64 execution does not establish native Linux hardware.

The Alint action file is an immutable-source retrieval for commit `1f235b300077205b05bf763f902aee274fdaccda`. Its SHA-256 is `8f060a6e3ca295308dbc2583dbb0240cbd5998f64b9775c5cb61303cefee8022`. It confirms the baked action input version is `v0.21.1`; these receipts do not claim that GitHub executed the action.
