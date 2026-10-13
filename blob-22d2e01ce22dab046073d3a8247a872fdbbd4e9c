# PR25 aggregate image and work-volume proof

This receipt covers the staged aggregate source tree `3f2e35a7eddc973650703c4c4838d8660ee99a53` (`HEAD=682a41afb7f4973b5be80764d6c271d2f665ad06`, `MERGE_HEAD=703d1b84d9f9e0246a52e3989bf3b78a13bbbf05`). It is local component evidence, not a live Scale Set or Actions qualification. Any later change to an image-context input requires a new build and probe. The incoming PR25 DinD seed change was not in this snapshot.

The 23 regular files used as build inputs, excluding `.dockerignore`, are recorded in [runner-work-volume-pr25-aggregate-context.sha256](runner-work-volume-pr25-aggregate-context.sha256). The newline-terminated manifest hashes to `175697eaa5306607e30079e9255b53b8a5b2b84a159a6f085a5f12bd515a2c8b`. Recompute it from the repository root with:

```sh
sha256sum -c docs/implemented/macos-scaleset/runner-work-volume-pr25-aggregate-context.sha256
sha256sum docs/implemented/macos-scaleset/runner-work-volume-pr25-aggregate-context.sha256
```

Both images were built for `linux/amd64` from Ubuntu 26.04 base image `sha256:f144425ff09be612d6d9ad965196e9cdc23dae1f42110a8a11a3e9a8198759f7`, using Docker Engine `29.8.2` on Linux x86_64 (engine ID `217d445c-e773-41da-b749-d0b32b0881e9`):

```sh
docker build --pull --progress=plain --platform linux/amd64 \
  -t velnor-dind:pr25-aggregate-3f2e35a7 images/dind
docker build --pull --progress=plain --platform linux/amd64 \
  -t velnor-runner:pr25-aggregate-3f2e35a7 images/runner/ubuntu-26.04
```

The immutable inspected images were DinD `sha256:48491940a5ad097481aacb6fcdf503a1533be8c7c7530f22cba3fece1e1c17ba` and runner `sha256:7905825c0dab8cc303392a79f762989e27b23cdf1d52464327c1766e4fe3c5c9`. Both inspect as `linux/amd64`. DinD runs `/usr/local/bin/velnor-dind-entrypoint`; the runner uses UID 1000, `/home/runner`, and `/usr/local/bin/velnor-runner-entrypoint`.

The installed DinD/runner entrypoints, socket wait and cleanup helpers, and tar tools were hashed inside the images; installed hashes match the manifest inputs. DinD's production `sh` entrypoint harness passed the ready, daemon-exit-before-socket, and no-socket cases; the failure cases published no public socket. Runner prerequisite checks and Dockerfile probes passed. The final image's tar suites passed: absolute-path suite 18/18 and semantics suite 23/23. Their logs are retained with the build receipts.

A fresh named volume was confirmed absent before creation, then created with `velnor.role=work` and a unique `velnor.worker` label. DinD's first mount initialized `/home/runner/_work` as `1000:1000 755`; its root process wrote a `0:0 644` marker. The default runner user `uid=1000(runner)` read that marker and wrote its own `1000:1000 644` marker. The actual runner entrypoint, given a dummy JIT body and one socket-wait attempt, exited 1 with exactly `docker socket missing`.

An instrumented `mktemp` observed a private `/tmp/velnor-jit.*` path with mode `600`; explicit chmod also set `600`. Container inspection confirmed the `_work` volume and the probe bind mount, with no mount at `/tmp`. The stopped entrypoint container did not contain the JIT tempfile. All named probe containers and the volume were absent after cleanup.

Raw source snapshot, context, build logs, image inspections, installed-file hashes, test logs, probe script, command output, and cleanup checks are retained under `/root/.cache/velnor-pr25-aggregate-image-3f2e35a7`. Key artifact hashes are:

| Artifact | SHA-256 |
| --- | --- |
| Build context manifest | `175697eaa5306607e30079e9255b53b8a5b2b84a159a6f085a5f12bd515a2c8b` |
| Source snapshot | `03ed4ef3bd91172baa86cd8d6361f21ef6ff9418b032b731370b3aa32f1bc310` |
| DinD build log | `5d11f788901aae2d5221516bb9e091d30868f2bdbcad4d3f278d52308c014650` |
| Runner build log | `25ff727caec192b26a6737f408f277e3fb7cca99f17c61514e16b1dc8a13beb4` |
| DinD image inspection | `56f92dd8511f285d8ec025e3a3e7a3c8da7064a8ab70ccd0187f6be4d2fcecd6` |
| Runner image inspection | `bb8fec575bc0bc0e424b8ac4eaf642fa7461fd75e42f927a8d2047a9a3ec5d64` |
| DinD readiness test log | `030227f0130dc152c1e37649d7c85d4b87221295c143a0a10d6b405a1d72d6b9` |
| Absolute tar test log | `e77f38e5c7200f66223a00d376888ac1ceed95e1d8a34850db8691d5c04d739e` |
| Tar semantics test log | `e217bbd29b78209cd5cd45eba7f281e744abe03a707d11cd80414dc524fcc088` |
| Fresh-volume probe script | `54d810c14d500b2d37bdb9e12224d56f7acff393d8b98e63901d6fc1ad5507a3` |
| Fresh-volume probe log | `855157a68b8c66c8ea59a59cd891d0de0db9a07918dd46ef5a0f39d61f90ed07` |

The receipt proves local image startup, mount ownership/access, temporary-file handling, tar behavior, and cleanup for this exact build context. It does not establish native macOS Keychain behavior, a live Scale Set run, workload cache behavior, or qualification completion.
