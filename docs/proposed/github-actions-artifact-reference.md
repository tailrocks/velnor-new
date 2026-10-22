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
