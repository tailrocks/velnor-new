# Runner work-volume contract proof

This records a source-bound check of the PR25 runner and DinD image inputs,
including the runner tar helpers present in that source snapshot. It verifies
the `_work` path and first-mount ownership on the local Docker engine. It does
not qualify a live Scale Set, start a DinD daemon, or run an Actions job.

## Path contract

The JIT request uses the `workFolder` field defined by the pinned
[`actions/scaleset` client and setting type](https://github.com/actions/scaleset/blob/e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5/client.go#L725-L743)
([field definition](https://github.com/actions/scaleset/blob/e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5/types.go#L93-L96)). In
`actions/runner` v2.337.0, `HostContext` resolves that relative setting beneath
the runner root, and the default work-folder constant is `_work`
([path resolution](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Common/HostContext.cs#L427-L435),
[constant](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Common/Constants.cs#L295)).
The checked-in JIT request therefore supplies `_work`; both runner and DinD
plans mount the same per-worker work volume at `/home/runner/_work`. The runner
image creates that directory as uid/gid `1000:1000`, and the DinD image seeds
it with uid/gid `1000:1000`, mode `0755`, before Docker's first empty-volume
copy-up. The one-use JIT tempfile remains in the runner container's private
`/tmp` and is outside this durable volume.

The nested contract test `runner_image_contract_tests` cross-checks the JIT
request, both container mount plans, image Dockerfiles, image documentation,
and the `/tmp` tempfile location. The Docker volume probe below independently
checks first-mount ownership and the default runner user's read/write access.

## Source and image identity

The tested working source snapshot was based on `0ab55efe4d121efc89fa5d72dbd5698dc87f5698`
plus the uncommitted runner correction listed in that worktree's status. The
combined Docker build-context manifest SHA-256 is
`14cafd4458d690d6a5bd04ba4eb6f954507948411c1c70175a28d0f9edcf5bce`, computed
over the exact 25 newline-terminated `sha256sum` entries printed in the raw
build receipt. The initial capture's `dac58359…` field did not match those
printed bytes; the original is retained as
`/tmp/runner25-work-volume-proof-initial-capture.log`, and the corrected final
receipt records the reproducible digest. Important source SHA-256 values:

| Input | SHA-256 |
|---|---|
| `images/dind/Dockerfile` | `9e6af678c45a8881d85327fce53e4a1d2549445f147d43214ebf3686b564cefc` |
| `images/dind/entrypoint.sh` | `75e153571a29bc9e192b99568845f95ee86ceee2940236298903019a5c8fb1c7` |
| `images/runner/ubuntu-26.04/Dockerfile` | `b33e893d1a0be6d819b009e22b0bbddf2d57180bb32fb5dcad3074c8d4789d76` |
| `images/runner/ubuntu-26.04/entrypoint.sh` | `7ba438aeaec3da7437b3cb23ced4a47425b55ce016dfcbcecc191db59c10d5ff` |
| `crates/velnor-runner/crates/velnor-runner-core/src/paths.rs` | `978bfc8c073c92b228588caf46754453a60b91ceef0aafb6301239b3b82158b4` |
| `crates/velnor-runner/crates/velnor-runner-github/src/session/config.rs` | `ea905dd7814b6d9f2e5922f049e4e546531220b13cfad5306f8da48b5229bfc5` |
| `crates/velnor-runner/crates/velnor-runner-host/src/docker_spec.rs` | `803ecc480f381e14b949b41e49ba177f8a2fddc008f4ec79927d867358cca414` |
| `crates/velnor-runner/crates/velnor-runner-host/src/worker/volumes.rs` | `5dd30839dc409beeda2d894fb757a55bd2ec216ecf26b500160610fd59d7b067` |
| `crates/velnor-runner/crates/velnor-runner-host/src/runner_image_contract_tests.rs` | `f391625ccc390eb9b18aace2cae91a726b9eaf17d27c151f3f60e6ffbf4036f5` |

The host contract test received a source-only assertion after the image build
to require that no runner mount targets `/tmp`; that test file is not in either
Docker context. Its final hash is shown above, and the focused contract tests
were rerun after the assertion was added. The first build capture records the
test's earlier hash; it does not change the image inputs or IDs.

Both images were built for `linux/amd64` from `ubuntu:26.04`, resolved to
`sha256:f144425ff09be612d6d9ad965196e9cdc23dae1f42110a8a11a3e9a8198759f7`.
The exact build commands were:

```sh
docker build --progress=plain --platform linux/amd64 \
  -t velnor-dind:pr25-contract-0ab55 images/dind
docker build --progress=plain --platform linux/amd64 \
  -t velnor-runner:pr25-contract-0ab55 images/runner/ubuntu-26.04
```

The immutable local image IDs used for the probe were verified with
`docker image inspect`:

| Image | Immutable ID | Relevant config |
|---|---|---|
| DinD | `sha256:e66de0ec9e4deb19a07ffe4257b2c397a297e3bcaa400d8680814ff7fe30d837` | `linux/amd64` |
| Runner | `sha256:edba0577b8504236626a46dd3d873fe51bd0a9181ab5bef3bfd072e4e08fdccb` | `linux/amd64`, user `runner`, workdir `/home/runner`, entrypoint `/usr/local/bin/velnor-runner-entrypoint` |

The source-bound Docker engine was `217d445c-e773-41da-b749-d0b32b0881e9`,
Engine `29.8.2`, Linux x86_64. The base image digest, context manifest, source
hashes, build output, image inspections, and the raw probe transcript are
retained in `/tmp/runner25-work-volume-proof-final.log` and
`/tmp/runner25-work-volume-proof-probe-final2.log` for this run. The final
probe also checked that its unique volume name was absent immediately before
creation, so the Docker copy-up observation comes from a newly created empty
volume. Earlier exploratory captures with reused volume names or stale image
IDs were superseded and retained separately under `/tmp`.

## Fresh-volume probe

A new volume labeled `velnor.role=work` and `velnor.worker=runner25-contract`
was first mounted at `/home/runner/_work` in the immutable DinD image. Docker
copied the image directory into the empty volume as `1000:1000 755`. A root
process in that container wrote a `0:0 644` marker. The immutable runner image
then mounted the same volume as its default user, `uid=1000(runner)`, read the
DinD marker, wrote a `1000:1000 644` marker, and read both markers. This
demonstrates the ownership and permission contract needed by the runner after
DinD initializes the shared named volume.

The real runner entrypoint was also invoked with a dummy `{}` JIT payload on
stdin and `WAIT_DOCKER_SOCK_TRIES=1`. It consumed the input and exited with the
expected `docker socket missing` error because this isolated check deliberately
did not attach a Docker socket. The subsequent runner-container check found no
`jit.*` file in the mounted work volume. Source and contract tests verify that
the tempfile is made under private `/tmp`, not under `_work`.

The probe commands below use the immutable IDs above. The three containers use
`--rm`; the entrypoint command's exit status 1 and `docker socket missing`
output are expected for this isolated probe.

```sh
volume=velnor-runner25-work-proof-final2-20261005
dind=sha256:e66de0ec9e4deb19a07ffe4257b2c397a297e3bcaa400d8680814ff7fe30d837
runner=sha256:edba0577b8504236626a46dd3d873fe51bd0a9181ab5bef3bfd072e4e08fdccb

if docker volume inspect "$volume" --format 'name={{.Name}}'; then exit 1; else echo 'expected: volume absent before create'; fi
docker volume create --label velnor.worker=runner25-contract --label velnor.role=work "$volume"
docker run --rm --network none --name runner25-dind-final2-20261005 --mount "type=volume,src=$volume,dst=/home/runner/_work" --entrypoint /bin/sh "$dind" -ec 'id; stat -c "%u:%g %a %n" /home/runner/_work; printf "dind-probe\\n" > /home/runner/_work/dind-probe; stat -c "%u:%g %a %n" /home/runner/_work/dind-probe'
docker run --rm --network none --name runner25-runner-final2-20261005 --mount "type=volume,src=$volume,dst=/home/runner/_work" --entrypoint /bin/sh "$runner" -ec 'id; stat -c "%u:%g %a %n" /home/runner/_work; test "$(cat /home/runner/_work/dind-probe)" = dind-probe; printf "runner-probe\\n" > /home/runner/_work/runner-probe; stat -c "%u:%g %a %n" /home/runner/_work/runner-probe; cat /home/runner/_work/dind-probe; cat /home/runner/_work/runner-probe'
set +e
entrypoint_output=$(printf '{}' | docker run --rm --network none --interactive --name runner25-entrypoint-final2-20261005 --env WAIT_DOCKER_SOCK_TRIES=1 --mount "type=volume,src=$volume,dst=/home/runner/_work" "$runner" 2>&1)
entrypoint_status=$?
set -e
printf '%s\n' "$entrypoint_output"
test "$entrypoint_status" -eq 1
test "$entrypoint_output" = 'docker socket missing'
docker run --rm --network none --name runner25-volume-final2-20261005 --mount "type=volume,src=$volume,dst=/home/runner/_work" --entrypoint /bin/sh "$runner" -ec 'if find /home/runner/_work -maxdepth 1 -name "jit.*" -print -quit | grep -q .; then exit 1; fi; stat -c "%u:%g %a %n" /home/runner/_work; cat /home/runner/_work/dind-probe; cat /home/runner/_work/runner-probe'
docker volume inspect "$volume" --format 'labels={{.Labels}}'
docker volume rm "$volume"
docker volume inspect "$volume" # expected to fail: removed
```

The observed values were `1000:1000 755 /home/runner/_work` on the first
mount, `1000:1000 644 /home/runner/_work/runner-probe` for the runner-created
file, and `map[velnor.role:work velnor.worker:runner25-contract]` for the
volume labels. The following container and volume inspections reported that
the resources were absent after cleanup:

```sh
docker container inspect runner25-dind-final2-20261005
docker container inspect runner25-runner-final2-20261005
docker container inspect runner25-entrypoint-final2-20261005
docker container inspect runner25-volume-final2-20261005
docker volume inspect velnor-runner25-work-proof-final2-20261005
```

All probe containers used `--rm`; container inspections confirmed none
remained. The labeled volume was removed and a subsequent inspection confirmed
it was absent. The transcript uses the exact immutable image IDs above and is
stored at `/tmp/runner25-work-volume-proof-probe-final2.log`.
