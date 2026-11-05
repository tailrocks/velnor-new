# GitHub Actions artifact upload reference

## Artifact upload missing-file semantics (checked 2026-10-22)

Velnor pins [`actions/upload-artifact` v7.0.2](https://github.com/actions/upload-artifact/tree/cf430e030ddbb5b0abf93d22962f4752f3646cd9).
At that exact source revision, [`upload-artifact.ts`](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/src/upload/upload-artifact.ts)
applies `if-no-files-found` only when the complete `filesToUpload` result is
empty; [`search.ts`](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/src/shared/search.ts)
resolves the supplied file, directory, or wildcard pattern and filters
directories before upload. A single action input containing multiple paths
therefore does not require every path to match. Task-suite artifact
declarations use one path pattern each and emit a separate pinned upload
action per declaration, so each required pattern gets its own
`if-no-files-found: error` check. The [pinned README](https://github.com/actions/upload-artifact/tree/cf430e030ddbb5b0abf93d22962f4752f3646cd9#not-uploading-to-the-same-artifact)
documents the no-files options and requirement for unique names across jobs.

## Raw evidence handoff (checked 2026-11-05)

The pinned [upload-artifact v7 action](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/action.yml) accepts `archive: false` for a single file and reports the uploaded artifact ID and SHA-256 digest. Its [bundled implementation](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/dist/upload/index.js) rejects multiple files in that mode. The pinned [download-artifact v8 action](https://github.com/actions/download-artifact/blob/9000827ccba6bdab643e8b6fd33ac0654aef8333/action.yml) accepts `skip-decompress: true` and defaults digest mismatches to errors. Its [bundled implementation](https://github.com/actions/download-artifact/blob/9000827ccba6bdab643e8b6fd33ac0654aef8333/dist/index.js) saves raw bytes before reporting the digest check; it does not impose Velnor's parser size limit on the transfer.

An empty `artifact-ids` input selects the unfiltered run artifact list rather than one artifact. Generated evidence downloads therefore validate a nonzero decimal artifact ID before invoking the action. The owned consumer bounds and validates the downloaded file before interpreting it.
