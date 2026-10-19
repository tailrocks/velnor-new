# Pinned upload ZIP compatibility fixture

`upload-artifact-v7.0.2-modes.zip` is a 332-byte, offline data fixture captured on
2026-10-19 from the actual bundled `createZipUploadStream` implementation in
`actions/upload-artifact` v7.0.2, commit
`cf430e030ddbb5b0abf93d22962f4752f3646cd9`.

The exact pinned `dist/upload/index.js` has Git blob identity
`7b3f92ae681d6eeca5f5e5e4cf892ad0dfb254ea` and SHA-256
`c0e58b240360573e6d6c82ea1bb22deff4f85883e00211a52bd27822c3a22617`.
Acquisition checked its bytes against the pinned GitHub tree. The only scratch
bundle change replaced the final action `run().catch(...)` invocation with an
export of the unchanged `createZipUploadStream`; its ZIP and bundled archiver
implementations were untouched. Node v24.21.0 on macOS executed this function
with network connections forbidden. No service upload or download occurred.

Both input files contain exactly `identical unprivileged fixture bytes` followed
by one LF (37 bytes, CRC-32 `4a477490`). Explicit source modes were 0600 and 0644.
The resulting raw ZIP retains Unix regular-file modes 0100600 and 0100644,
respectively, with DEFLATE and streaming data descriptors for both entries.
It demonstrates raw ZIP behavior, not download-action permission normalization,
hosted qualification, artifact authentication, or release provenance.

Fixture SHA-256:
`9ebbbf81eaa69fa82acac34953ac5e623578fff91c8b038d150cbefe14a326b1`.
The capture report and original source bytes are retained at
`/Users/donbeave/.cache/velnor/upload-artifact-mode-20261019-Y3CvPK/`.
Tests admit the exact mixed-mode inventory, validate its content and headers,
reject an all-0644 expectation, and reject a consistent-metadata CRC mutation.
The native upload contract remains exact 0644; source fixture mode 0600 is
intentionally negative for that contract.
