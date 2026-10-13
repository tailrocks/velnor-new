# Runner work-volume contract proof

This file preserves the pre-PR48 PR25 receipt and records a rebuilt check of
the merged PR25/PR48 image inputs. The earlier image IDs are historical and
are superseded by the final combined receipt below. Neither local check
qualifies a live Scale Set, starts a DinD daemon, or runs an Actions job.
The separate PR48-only image receipt is preserved in
[`runner-work-volume-pr48-proof.md`](runner-work-volume-pr48-proof.md).

## Earlier PR25-only image receipt (superseded)

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

After the `6180ccebc` main sync and normal PR25 merge of `4e843d6e`, a rehash
matched all 25 captured inputs and reproduced this manifest digest. The later
`pins.rs` retry change and qualification/evidence updates are outside both
image build contexts. The recorded immutable image IDs and fresh-volume probe
therefore remain bound to the same runner and DinD bytes; no image rebuild or
probe rerun was needed.

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

## Earlier merged PR25 + PR48 image receipt (superseded below)

The `b3aec0818fadfcccafe7c4aaa6b0ea7474dd8e76` merge changed the runner
entrypoint and image documentation. I rebuilt both combined images and
repeated the new-empty-volume check against their immutable IDs. The source
manifest below contains every regular file in both build contexts except
`.dockerignore`, sorted by path and newline-terminated. Its exact SHA-256 is
`5377cb5d6572fb7ddc67276247265dcddc3a4a8aff54391826c9f43601bb6349`.

Regenerate and check the manifest with:

```sh
find images/dind images/runner/ubuntu-26.04 -type f ! -name .dockerignore -print0 \
  | LC_ALL=C sort -z \
  | xargs -0 sha256sum > /tmp/runner25-main-b3-context.sha256
sha256sum /tmp/runner25-main-b3-context.sha256
```

```text
9e6af678c45a8881d85327fce53e4a1d2549445f147d43214ebf3686b564cefc  images/dind/Dockerfile
a20a6cc25ced02df1ba542aefc896150818d2dde8a55058ab144d8eb586ec35f  images/dind/README.md
756962f1ff22bedfd2cfce04fe14a843b518fb54e9f9b972b90550462fa03402  images/dind/entrypoint-test.sh
75e153571a29bc9e192b99568845f95ee86ceee2940236298903019a5c8fb1c7  images/dind/entrypoint.sh
b33e893d1a0be6d819b009e22b0bbddf2d57180bb32fb5dcad3074c8d4789d76  images/runner/ubuntu-26.04/Dockerfile
00a4bcef73aafe4be2ff61242576ec4d7de0e4249f86b6691574dff34b02202a  images/runner/ubuntu-26.04/README.md
95746eb84e0896d408eea6afd4dd1eb0a9571b122445352f815fd2d3a98f1171  images/runner/ubuntu-26.04/clear-dockerenv-test.sh
4e8fcacb646c4fe61b5ba4f1cf1e376e5a4a641f79e5097dd920e5e06d363f52  images/runner/ubuntu-26.04/clear-dockerenv.sh
8dbf0ef0200055c2a1d4c4dd0f45ac384690139de1b92c2b568e666de8ca2673  images/runner/ubuntu-26.04/entrypoint.sh
67916fc6eefb9e94fedde1b8d27fd4cb5c1d540c489de0538344c9f5a0f84a91  images/runner/ubuntu-26.04/runner-job-env-test.sh
c40231f0247d9f79576ce78f50d46e68eff1b3bb2df39eaebeacedc8d7509410  images/runner/ubuntu-26.04/runner-job-env.sh
a4ada6e2840a4c0a6305c7c9c891100b3c65e8471141eab4a9188e4dfe471000  images/runner/ubuntu-26.04/tar-absolute-test-extended.sh
9ff9341a7dc96413ac6284bd328cec82b5d4676fd75c599660aa11b7f21d9bac  images/runner/ubuntu-26.04/tar-absolute-test.sh
cf1a41eb021d6287348effb5bc61a33253ed9c3e9ed73ee93c7c3bdbd58ce172  images/runner/ubuntu-26.04/tar-absolute.sh
9e3fe7b805b6f50b29a5e5e6edaf7373dc0e8e805885d4008c99d449ec9ad40d  images/runner/ubuntu-26.04/tar-extract-plan.sh
c3b811fa95396723947eb170cd5b359af0b001b4bf2ff009f223f4095d46a5e2  images/runner/ubuntu-26.04/tar-extract.sh
00c5649fc19c1479805e94d0e5d9991855f7b7630d822efbd57b99657cccbf12  images/runner/ubuntu-26.04/tar-member-rewrite.pl
bf7a376b0e161c2cdf8c0065f2ae92542ec64b24470c367581308ff4c097de20  images/runner/ubuntu-26.04/tar-member-stream.pl
eafebc28c0643148b7ca21b3037ce7f487db9444f6be7312a227a297aa064ccd  images/runner/ubuntu-26.04/tar-member.pl
897516dbaf91a2413ee74ccde6f1b2b2e2fd90506aaa5cb750df2fb0e5e32915  images/runner/ubuntu-26.04/tar-pax.pl
e575ae760d87ef630505f380ec8e20fc6ae292736281e82588062cfd82982759  images/runner/ubuntu-26.04/tar-semantics-planned-test.sh
2ba20ed2628a5d0ad1a31302b56e2c398ba35bff8ec8d9f72ebe8bfd6ee8408f  images/runner/ubuntu-26.04/tar-semantics-test.sh
19c7c1f10c78d14dfffc70fa33f2522e1131b5b4f7d188250bdca0c81e3cd0d2  images/runner/ubuntu-26.04/tar-shim.sh
37558be915ac85228df4789ce2a95486edd092af2a942b9dffd5a218228d34ab  images/runner/ubuntu-26.04/wait-docker-sock-test.sh
f6ba95c71712ef7c1b382877503433b545035df87f83cef95d416f126c996a29  images/runner/ubuntu-26.04/wait-docker-sock.sh
```

The pinned base was `ubuntu:26.04`, image ID and manifest digest
`sha256:f144425ff09be612d6d9ad965196e9cdc23dae1f42110a8a11a3e9a8198759f7`.
Docker Engine was `29.8.2`, Linux x86_64, ID
`217d445c-e773-41da-b749-d0b32b0881e9`. The exact rebuild commands were:

```sh
docker build --pull --progress=plain --platform linux/amd64 \
  -t velnor-dind:pr25-b3-combined-20261005 images/dind
docker build --pull --progress=plain --platform linux/amd64 \
  -t velnor-runner:pr25-b3-combined-20261005 images/runner/ubuntu-26.04
```

| Image | Immutable ID | Inspected configuration |
|---|---|---|
| DinD | `sha256:9ef60885d75d21869db5a4f1a190a9020acba2a1798a430588d447cd0b50ded4` | `linux/amd64`, entrypoint `/usr/local/bin/velnor-dind-entrypoint` |
| Runner | `sha256:bcb0413d6ad3510293543a67095c1f7314c9f455a6896d4d339b83effbe4f065` | `linux/amd64`, user `runner`, workdir `/home/runner`, entrypoint `/usr/local/bin/velnor-runner-entrypoint` |

The fresh volume was explicitly absent before creation. DinD's first mount
copied up `/home/runner/_work` as `1000:1000 755`. The default runner user
`uid=1000(runner)` read the DinD marker, wrote a `1000:1000 644` file, and read
both markers. The actual entrypoint consumed a dummy JIT payload and exited
with the expected `docker socket missing` status because this probe supplied
no socket. A follow-up check found neither `jit.*` nor `velnor-jit.*` under
the durable work volume. All four named containers were absent after their
`--rm` runs, and the volume was removed and then confirmed absent.

The reproducible probe invocation, including the immutable IDs and container
names used for cleanup inspection, was:

```sh
set -euo pipefail
volume=velnor-runner25-b3-work-proof-20261005
dind=sha256:9ef60885d75d21869db5a4f1a190a9020acba2a1798a430588d447cd0b50ded4
runner=sha256:bcb0413d6ad3510293543a67095c1f7314c9f455a6896d4d339b83effbe4f065
cleanup() {
  docker rm -f runner25-b3-dind-proof-20261005 runner25-b3-runner-proof-20261005 runner25-b3-entrypoint-proof-20261005 runner25-b3-volume-proof-20261005 >/dev/null 2>&1 || true
  docker volume rm "$volume" >/dev/null 2>&1 || true
}
trap cleanup EXIT
if docker volume inspect "$volume" --format 'name={{.Name}}'; then exit 1; else printf 'expected: volume absent before create\n'; fi
docker volume create --label velnor.worker=runner25-b3-proof --label velnor.role=work "$volume"
docker run --rm --network none --name runner25-b3-dind-proof-20261005 --mount "type=volume,src=$volume,dst=/home/runner/_work" --entrypoint /bin/sh "$dind" -ec 'id; stat -c "%u:%g %a %n" /home/runner/_work; printf "dind-probe\n" > /home/runner/_work/dind-probe; stat -c "%u:%g %a %n" /home/runner/_work/dind-probe'
docker run --rm --network none --name runner25-b3-runner-proof-20261005 --mount "type=volume,src=$volume,dst=/home/runner/_work" --entrypoint /bin/sh "$runner" -ec 'id; stat -c "%u:%g %a %n" /home/runner/_work; test "$(cat /home/runner/_work/dind-probe)" = dind-probe; printf "runner-probe\n" > /home/runner/_work/runner-probe; stat -c "%u:%g %a %n" /home/runner/_work/runner-probe; cat /home/runner/_work/dind-probe; cat /home/runner/_work/runner-probe'
if entrypoint_output=$(printf '{}' | docker run --rm --network none --interactive --name runner25-b3-entrypoint-proof-20261005 --env WAIT_DOCKER_SOCK_TRIES=1 --mount "type=volume,src=$volume,dst=/home/runner/_work" "$runner" 2>&1); then entrypoint_status=0; else entrypoint_status=$?; fi
printf '%s\n' "$entrypoint_output"
printf 'entrypoint_status=%s\n' "$entrypoint_status"
test "$entrypoint_status" -eq 1
test "$entrypoint_output" = 'docker socket missing'
docker run --rm --network none --name runner25-b3-volume-proof-20261005 --mount "type=volume,src=$volume,dst=/home/runner/_work" --entrypoint /bin/sh "$runner" -ec 'if find /home/runner/_work -maxdepth 1 \( -name "jit.*" -o -name "velnor-jit.*" \) -print -quit | grep -q .; then exit 1; fi; stat -c "%u:%g %a %n" /home/runner/_work; cat /home/runner/_work/dind-probe; cat /home/runner/_work/runner-probe'
docker volume inspect "$volume" --format 'labels={{.Labels}}'
for name in runner25-b3-dind-proof-20261005 runner25-b3-runner-proof-20261005 runner25-b3-entrypoint-proof-20261005 runner25-b3-volume-proof-20261005; do
  if docker container inspect "$name" >/dev/null 2>&1; then exit 1; else printf 'expected: container absent: %s\n' "$name"; fi
done
docker volume rm "$volume"
trap - EXIT
if docker volume inspect "$volume" --format 'name={{.Name}}'; then exit 1; else printf 'expected: volume absent after cleanup\n'; fi
```

The entrypoint invocation is expected to exit 1 with exactly `docker socket
missing`; the other probe commands pass. The complete build and probe receipts
are retained at `/tmp/runner25-b3-image-build.log` and
`/tmp/runner25-b3-work-volume-probe.log`. The nested source contract test also
checks the `_work` mount, private `/tmp/velnor-jit.*` location, and absence of
a runner mount at `/tmp`. Its source SHA-256 is
`6e27926566362fb05ca792437ed8a148644c31589cfbdb56af77745620c3fe04`.
## Earlier helper-branch image receipt

This receipt applies to the separately reviewed helper branch before the
aggregate main/PR sync. The frozen aggregate image proof is in
[runner-work-volume-pr25-aggregate-proof.md](runner-work-volume-pr25-aggregate-proof.md).

This build combines the runner-image correction reviewed against base
`54a1f2e6033ab10c7d38b3cf51378b05b93920c2` (binary diff SHA-256
`62e7cb28185192fdcfeb582fdcf81033bda47e854cc1d86f3ce679035f04d815`) with
the separately reviewed tar fix `0fbc621f7da351d809466d25ad821569ded06c96`.
The DinD entrypoint no longer pulls a mutable RabbitMQ workload image; it
publishes the private Docker socket only after creation, and fails nonzero if
the daemon exits before readiness or does not create the socket within about
10 seconds. The runner image no longer sets `CARGO_BUILD_JOBS` from host
`nproc`; workflow configuration owns compile parallelism. This receipt covers
local image and volume behavior only, not a live Scale Set or Actions job.

The combined build context includes all 23 regular files in the two image
directories, excluding `.dockerignore`. The newline-terminated sorted
`sha256sum` manifest is
`/root/.cache/velnor-pr25-image-context-final.sha256` and hashes to
`83544967e64300a68a4969c119e0d8353965e8cf9017d862375315692bd9e7ed`:

```text
9e6af678c45a8881d85327fce53e4a1d2549445f147d43214ebf3686b564cefc  images/dind/Dockerfile
156d4353834a3e66ac09bd18162581dd6db8eabc59e5c76040881e6d6aeb64de  images/dind/README.md
56adac08fbb9b7b522ea257a676e6cc5d5c53e54f60099199e719c3d039cdaa7  images/dind/entrypoint-test.sh
9be9c89d96ba5feb69282d8aaf9e978cf2dba9aa117293ba394b44346875f3d5  images/dind/entrypoint.sh
6fc16a1877d405c2afcd29875f5950f9ca0fad3648c30a63b9b7d74065525e49  images/runner/ubuntu-26.04/Dockerfile
9b47040fbbde2e7bd56a76eb874f514cd0fa2318a597e77d2ccfebf64f828960  images/runner/ubuntu-26.04/README.md
95746eb84e0896d408eea6afd4dd1eb0a9571b122445352f815fd2d3a98f1171  images/runner/ubuntu-26.04/clear-dockerenv-test.sh
4e8fcacb646c4fe61b5ba4f1cf1e376e5a4a641f79e5097dd920e5e06d363f52  images/runner/ubuntu-26.04/clear-dockerenv.sh
6f245b32f88164fa9390fbee5ba0c4a3c42932cb4fdd5be81fb9e4181fab855c  images/runner/ubuntu-26.04/entrypoint.sh
a4ada6e2840a4c0a6305c7c9c891100b3c65e8471141eab4a9188e4dfe471000  images/runner/ubuntu-26.04/tar-absolute-test-extended.sh
335a2cea5105ac6dc8a08d337f67d394274a23ecb371913f65d071bc0bddea18  images/runner/ubuntu-26.04/tar-absolute-test.sh
28a654acaaa1cfd0df16229c908980be3c9a33c0c9c3ba664243c392394dd42d  images/runner/ubuntu-26.04/tar-absolute.sh
9e3fe7b805b6f50b29a5e5e6edaf7373dc0e8e805885d4008c99d449ec9ad40d  images/runner/ubuntu-26.04/tar-extract-plan.sh
c3b811fa95396723947eb170cd5b359af0b001b4bf2ff009f223f4095d46a5e2  images/runner/ubuntu-26.04/tar-extract.sh
00c5649fc19c1479805e94d0e5d9991855f7b7630d822efbd57b99657cccbf12  images/runner/ubuntu-26.04/tar-member-rewrite.pl
bf7a376b0e161c2cdf8c0065f2ae92542ec64b24470c367581308ff4c097de20  images/runner/ubuntu-26.04/tar-member-stream.pl
eafebc28c0643148b7ca21b3037ce7f487db9444f6be7312a227a297aa064ccd  images/runner/ubuntu-26.04/tar-member.pl
897516dbaf91a2413ee74ccde6f1b2b2e2fd90506aaa5cb750df2fb0e5e32915  images/runner/ubuntu-26.04/tar-pax.pl
c31f6f74764ed80e2f0407fcb461e52b6562506b8b49bd81d4a203cc2dd8045f  images/runner/ubuntu-26.04/tar-semantics-planned-test.sh
491d23bf80be47f497569ec4c77ee7b3f0ca71d2298fbab45e878f339b11a1e1  images/runner/ubuntu-26.04/tar-semantics-test.sh
de9d08ab1cfe01021ca975286b42d2a2c648257066d7088b58c684c195623ab4  images/runner/ubuntu-26.04/tar-shim.sh
37558be915ac85228df4789ce2a95486edd092af2a942b9dffd5a218228d34ab  images/runner/ubuntu-26.04/wait-docker-sock-test.sh
78f55886c234691b6587a745e99e75ae6ded3666bb3c2a6184288ace65d45577  images/runner/ubuntu-26.04/wait-docker-sock.sh
```

Both builds used `ubuntu:26.04` at
`sha256:f144425ff09be612d6d9ad965196e9cdc23dae1f42110a8a11a3e9a8198759f7`
on Docker Engine `29.8.2`, Linux x86_64, engine ID
`217d445c-e773-41da-b749-d0b32b0881e9`. The exact commands were:

```sh
docker build --pull --progress=plain --platform linux/amd64 \
  -t velnor-dind:pr25-image-fix-20261005 images/dind
docker build --pull --progress=plain --platform linux/amd64 \
  -t velnor-runner:pr25-image-fix-20261005 images/runner/ubuntu-26.04
```

The final inspected image IDs and configuration are:

| Image | Immutable ID | Configuration |
|---|---|---|
| DinD | `sha256:86c7f1e94f92db13f881bdca3cf1055cf329bc25a855dc43c7451c94d99c8274` | `linux/amd64`, entrypoint `/usr/local/bin/velnor-dind-entrypoint` |
| Runner | `sha256:8c0e114a306c3bb85715007787514bb9adb4f133305f02c8808128ff23688f03` | `linux/amd64`, user `runner`, workdir `/home/runner`, entrypoint `/usr/local/bin/velnor-runner-entrypoint` |

The installed DinD and runner entrypoints, socket-wait helper, and tar shim
were hashed inside those exact images; all installed hashes match their source
files. The DinD readiness harness ran with the image's current DIND source
bind-mounted and a stub `dockerd`. It passed the ready case with no Docker
command invocation, a `dockerd` exit-0-before-socket case that exited 1, and
a no-socket case guarded by `timeout 15s`; neither failure case published the
public socket. In the final runner image, `wait-docker-sock-test.sh`,
`clear-dockerenv-test.sh`, and the DinD readiness harness passed. The
Dockerfile's own tar probes also completed during the runner build. Separate
tar shell suites passed 18/18 and 22/22 with logs under
`/root/.cache/velnor-pr25-tar-strip-test-logs/`.

A fresh-volume probe used the immutable IDs above and volume
`velnor-pr25-work-imagefix-20261005-01`. The volume was confirmed absent before
creation, then labeled `velnor.role=work` and
`velnor.worker=runner25-imagefix`. DinD's first mount copied `/home/runner/_work`
as `1000:1000 755`; its root process wrote a `0:0 644` marker. The default
runner user `uid=1000(runner)` read that marker and wrote a `1000:1000 644`
marker. A wrapper invoked the actual runner entrypoint with a dummy JIT body
and one socket-wait attempt; it exited 1 with `docker socket missing`. An
exported `mktemp` observer recorded a temporary path under `/tmp/velnor-jit.*`,
verified that file was removed, and verified that no JIT file appeared in the
work volume. Container inspection showed only the `_work` volume mount on
every probe container, with no `/tmp` mount. All named containers and the
volume were removed and then confirmed absent.

The raw build logs, installed-file hash check, component-test transcript,
probe script, and probe output are retained in `/root/.cache` as
`velnor-pr25-imagefix-dind-build.log`,
`velnor-pr25-imagefix-runner-build.log`,
`runner25-imagefix-installed-files.log`,
`runner25-imagefix-shell-tests.log`,
`runner25-work-volume-probe-final.sh`,
`runner25-work-volume-probe-inner.sh`, and
`runner25-imagefix-work-volume-probe.log`. Their SHA-256 values are:

| Receipt artifact | SHA-256 |
|---|---|
| Context manifest | `83544967e64300a68a4969c119e0d8353965e8cf9017d862375315692bd9e7ed` |
| DinD build log | `79bf22bd19cb7c7965fd7eba2a5cbe1c8419ab18c4f93e947d859bdcaf4dac8a` |
| Runner build log | `36e6303a2608fb285cd0e7c45b66e794017aa14c9a0ba779128527cfe08061b3` |
| Installed-file hash log | `d51578d746bbfb487204a442cd262568ed69291ebc6a256c699b068e4e2c235d` |
| Component-test log | `e5b1af174dbdb81a60fdb09e38039b8c55bae251cf3e9a38fa927b7dff58b682` |
| Probe script | `fcc954966eb0005c4d574ab68cb2de46407960327a627ac8d804f76c61223353` |
| Inner JIT probe | `db92bed654bbbf9ade06e277b0eb1e0a8669f79df43cc8ee36a287d673b90af2` |
| Probe output log | `b167f6139573ebcc746f15fb0793c86155a93753f199b2202288e420d97e21f4` |

The nested
`runner_image_contract_tests` source is SHA-256
`6c474faf2a3ef92dcd340c2efe444f3433db251e2db0086c89d369a5eadcece1`;
the three focused tests passed, along with nested host Clippy and formatting,
Shellcheck 0.11, `bash -n`, and `git diff --check`. This evidence does not
qualify native macOS Keychain behavior, a live Scale Set, or a workflow job.
