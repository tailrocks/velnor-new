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

## Artifact download extraction boundary

Velnor pins [`actions/download-artifact` v8.0.2](https://github.com/actions/download-artifact/tree/9000827ccba6bdab643e8b6fd33ac0654aef8333).
Its [download step](https://github.com/actions/download-artifact/blob/9000827ccba6bdab643e8b6fd33ac0654aef8333/src/download-artifact.ts)
uses the artifact service digest and defaults `digest-mismatch` to `error`.
The pinned [dependency lock](https://github.com/actions/download-artifact/blob/9000827ccba6bdab643e8b6fd33ac0654aef8333/package-lock.json)
selects `@actions/artifact` 6.3.1 and `unzip-stream` 0.3.4. The pinned action's [bundled download implementation](https://github.com/actions/download-artifact/blob/9000827ccba6bdab643e8b6fd33ac0654aef8333/dist/index.js#L126654-L126706)
streams ZIP extraction before reporting a digest mismatch. Its bundled
[extractor](https://github.com/actions/download-artifact/blob/9000827ccba6bdab643e8b6fd33ac0654aef8333/dist/index.js#L67461-L67471)
joins member paths to the destination without a containment check. Digest verification after extraction does not
bound extraction or protect against a hostile archive member. Workflows that
use this action must establish producer and artifact identity before download
and may not claim pre-extraction path or byte screening by the action.
