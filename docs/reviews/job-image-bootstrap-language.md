# Job image bootstrap wording corrections

This note records corrected wording for three immutable commit messages. The commit objects remain unchanged.

External evidence: The text in the `Previous body` column is from earlier commits. It is unchanged.

| Commit | Previous body | Corrected wording |
| --- | --- | --- |
| `a54bfaa73b7a192295ea0d2ceee03e97bef5d280` | Restore the missing test capture function. Check nested calls and error recovery. | Repair the missing test capture function. Do a check of nested calls and error recovery. |
| `906b9c86796175f823fdf28090b166749868b4b2` | Build and attest the Linux job image. Permit publication only from a manual run on main. Check source and artifact identity before publication. | Build and attest the Linux job image. Publish only when CI conclusion is `success` for the same source commit on `main`. Do a check of the source and artifact identities before publication. |
| `bce95704f1f02fb9ffc00b3f4ac1c935cfa6fa67` | Generate the dogfood files with the current source. Check time limits for the new workflow. | Generate the dogfood files from the current source. Do a check of the new workflow time limits. |
