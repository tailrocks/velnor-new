# Runner work-volume image contract proof

Observed 2026-10-05 on Docker Engine `29.8.2`, Linux `x86_64`. This receipt
covers the PR48 source hashes below only; it does not qualify a separately
changed PR25 image. It records a local image and empty-volume check, not a live
GitHub Scale Set qualification.

The pinned [actions/runner v2.337.0 `HostContext`](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Common/HostContext.cs#L427-L436)
resolves Work by joining the runner root with `settings.WorkFolder`; the
default is `_work` at [`Constants.cs` line 295](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Common/Constants.cs#L295).
The pinned [actions/scaleset JIT client](https://github.com/actions/scaleset/blob/e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5/client.go#L725-L743)
marshals the JIT settings into the request body, and [`types.go`](https://github.com/actions/scaleset/blob/e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5/types.go#L93-L96)
defines `workFolder` as a string. With the runner root at `/home/runner`,
this implementation's explicit `_work` resolves to `/home/runner/_work`.

The built images were:

- Base `ubuntu:26.04`, manifest digest `sha256:f144425ff09be612d6d9ad965196e9cdc23dae1f42110a8a11a3e9a8198759f7`.
- DinD `velnor-dind:29.8.2`, image ID `sha256:c75f47ba6049197aa3ca0161bcaaf29eb4756b6c63f370e1e65bfec234d49395`.
- Runner `velnor-runner:ubuntu-26.04-2.337.0`, image ID `sha256:28183f6f55daa486d1901415ffcf6d884930bed120ab7a70e57dced77789ec46`, configured user `runner`, work directory `/home/runner`, entrypoint `/usr/local/bin/velnor-runner-entrypoint`.

The exact source digests used for the image and contract check are:

| Source | SHA-256 |
| --- | --- |
| `images/dind/Dockerfile` | `c98c72b594e707e57bb8afaee472867694e206f148520dc8d1102eb18047d6c8` |
| `images/runner/ubuntu-26.04/Dockerfile` | `bf02fd1fe2e424bc135e0cbea46363a4996f3418b6e958330a8a89f1c72376c9` |
| `images/runner/ubuntu-26.04/entrypoint.sh` | `7eacc37d90ac65b4c750db7a5dc1062510be0eea38b39a99400077c64d94ca49` |
| `images/runner/ubuntu-26.04/tar-shim.sh` | `91099207ace530c5853429291d93188353a1d5cffaf87957de1c99d22b8eb245` |
| `images/dind/README.md` | `8930c3f6b7f5a1a01492e65ca5a1d1713371ab967b9cca74e6a17da7a8e2e76a` |
| `images/runner/ubuntu-26.04/README.md` | `72796cdfde4eb650c61d11ecf0820ea7902a0da16cd320f5565dbf9072b8e1b9` |
| `crates/velnor-runner/crates/velnor-runner-core/src/paths.rs` | `978bfc8c073c92b228588caf46754453a60b91ceef0aafb6301239b3b82158b4` |
| `crates/velnor-runner/crates/velnor-runner-github/src/session/config.rs` | `05e15110faa0feefccca8229cae7bde78877855b08b93046bdd77deb25938be4` |
| `crates/velnor-runner/crates/velnor-runner-host/src/docker_spec.rs` | `803ecc480f381e14b949b41e49ba177f8a2fddc008f4ec79927d867358cca414` |
| `crates/velnor-runner/crates/velnor-runner-host/src/worker/volumes.rs` | `8c7a84d779e6747ff695c3fd1c146accb60725aca8fe27b1b962e4e6e4ca8c79` |
| `crates/velnor-runner/crates/velnor-runner-host/src/runner_image_contract_tests.rs` | `6e27926566362fb05ca792437ed8a148644c31589cfbdb56af77745620c3fe04` |
| `crates/velnor-runner/crates/velnor-runner-host/src/worker/volumes_tests.rs` | `6088c0e85bc589f0f35e6e60a7a4989b9cf1498bb291fa535a634ade435762e9` |
| `docs/proposed/macos-scaleset-runner.md` | `62366616a5e0b6a44c16bb6b755641cbbb1197207d1f700420328b39ce1ba69d` |

The entrypoint creates its mode-`0600` JIT scratch file at
`/tmp/velnor-jit.XXXXXX`, outside the worker work volume. The source-bound
contract test asserts that path and verifies `/tmp` is not a runner mount.

The images were rebuilt from these checked-in Dockerfiles after pulling the
base manifest above:

```sh
docker pull ubuntu:26.04
docker build --platform linux/amd64 -t velnor-dind:29.8.2 images/dind
docker build --platform linux/amd64 -t velnor-runner:ubuntu-26.04-2.337.0 images/runner/ubuntu-26.04
```

The source hash, image ID and engine commands for the fresh-volume check were:

```sh
sha256sum images/dind/Dockerfile images/runner/ubuntu-26.04/Dockerfile \
  images/runner/ubuntu-26.04/entrypoint.sh images/runner/ubuntu-26.04/tar-shim.sh \
  images/runner/ubuntu-26.04/README.md \
  crates/velnor-runner/crates/velnor-runner-host/src/runner_image_contract_tests.rs \
  docs/proposed/macos-scaleset-runner.md
docker image inspect velnor-dind:29.8.2 --format '{{.Id}}'
docker image inspect velnor-runner:ubuntu-26.04-2.337.0 --format '{{.Id}} {{.Os}}/{{.Architecture}} {{.Config.User}} {{.Config.WorkingDir}} {{json .Config.Entrypoint}}'
volume=velnor-runner48-proof-final-20261005
docker volume create --label velnor.worker=runner48-proof --label velnor.role=work "$volume"
docker run --rm --volume "$volume:/home/runner/_work" --entrypoint /bin/sh \
  velnor-dind:29.8.2 -ec 'id; stat -c "%u:%g %a %n" /home/runner/_work; printf "dind-ok\\n" > /home/runner/_work/dind-probe; stat -c "%u:%g %a %n" /home/runner/_work/dind-probe'
runner_status=0
docker run --rm -i --volume "$volume:/home/runner/_work" \
  velnor-runner:ubuntu-26.04-2.337.0 </dev/null || runner_status=$?
test "$runner_status" -eq 2
printf 'empty-payload-entrypoint-status=%s\n' "$runner_status"
docker run --rm --volume "$volume:/home/runner/_work" --entrypoint /bin/sh \
  velnor-runner:ubuntu-26.04-2.337.0 -ec 'id; stat -c "%u:%g %a %n" /home/runner/_work; test "$(cat /home/runner/_work/dind-probe)" = dind-ok; test -z "$(find /home/runner/_work -maxdepth 1 -name "jit.*" -print -quit)"; printf "runner-ok\\n" > /home/runner/_work/runner-probe; stat -c "%u:%g %a %n" /home/runner/_work/runner-probe; cat /home/runner/_work/dind-probe'
docker volume inspect "$volume" --format 'labels={{.Labels}}'
docker volume rm "$volume"
if docker volume inspect "$volume" >/dev/null 2>&1; then exit 1; fi
```

The fresh volume was first mounted into the DinD image at `/home/runner/_work`,
then into the runner image at that same target. Its initialization copy-up
produced `1000:1000`, mode `0755`; DinD ran as root and created a `0:0`, mode
`0644` marker. The empty-payload entrypoint exited with status 2, and a
subsequent inspection found no `jit.*` file in the named work volume. The
runner's default `uid=1000(runner)` read the marker and wrote a file as
`1000:1000`, mode `0644`. The engine reported the expected
`velnor.worker=runner48-proof` and `velnor.role=work` labels. The final inspect
assertion confirmed that the volume was absent after removal. This probe covers
normal empty-input cleanup and work-volume separation; it does not test a
forced termination of the runner container.

Observed output:

```text
uid=0(root) gid=0(root) groups=0(root)
1000:1000 755 /home/runner/_work
0:0 644 /home/runner/_work/dind-probe
empty jit
empty-payload-entrypoint-status=2
uid=1000(runner) gid=1000(runner) groups=1000(runner),999(docker)
1000:1000 755 /home/runner/_work
1000:1000 644 /home/runner/_work/runner-probe
dind-ok
labels=map[velnor.role:work velnor.worker:runner48-proof]
velnor-runner48-proof-final-20261005
cleanup=confirmed volume=velnor-runner48-proof-final-20261005 absent
```

The full command and output transcript is
`/tmp/runner48-work-volume-proof-final.log`.
The source-bound tests additionally assert the JIT request value, resolved
path, both container mount plans, runner installation and entrypoint paths,
the `/tmp` JIT scratch path with no work-volume mount there, UID 1000 image
setup, DinD volume initialization, and worker/role labels.

## Fresh-volume absence witness

The first probe above did not record an inspection before its named-volume
create. This follow-up closes that evidence gap. On the same Docker Engine, it
uses the same immutable DinD and runner image IDs above and a distinct volume
name. Both image IDs were inspected before the test. The pre-create inspect
returned status 1 and Docker's `no such volume` response before `docker volume
create`; the volume was then mounted first into DinD and next into the runner.

The exact commands were:

```sh
set -Eeuo pipefail
volume=velnor-runner48-proof-freshness-20261005
worker=runner48-proof-freshness-20261005
dind_image=sha256:c75f47ba6049197aa3ca0161bcaaf29eb4756b6c63f370e1e65bfec234d49395
runner_image=sha256:28183f6f55daa486d1901415ffcf6d884930bed120ab7a70e57dced77789ec46
docker image inspect "$dind_image" --format 'dind={{.Id}} {{.Os}}/{{.Architecture}}'
docker image inspect "$runner_image" --format 'runner={{.Id}} {{.Os}}/{{.Architecture}} {{.Config.User}} {{.Config.WorkingDir}} {{json .Config.Entrypoint}}'
if docker volume inspect "$volume" > /tmp/runner48-volume-freshness-preinspect-20261005.txt 2>&1; then
  cat /tmp/runner48-volume-freshness-preinspect-20261005.txt
  exit 1
else
  preinspect_status=$?
  printf 'precreate-inspect-status=%s\n' "$preinspect_status"
  cat /tmp/runner48-volume-freshness-preinspect-20261005.txt
  test "$preinspect_status" -eq 1
  grep -F 'no such volume' /tmp/runner48-volume-freshness-preinspect-20261005.txt
fi
docker volume create --label "velnor.worker=$worker" --label velnor.role=work "$volume"
docker run --rm --volume "$volume:/home/runner/_work" --entrypoint /bin/sh "$dind_image" -ec 'id; stat -c "%u:%g %a %n" /home/runner/_work; printf "dind-ok\\n" > /home/runner/_work/dind-probe; stat -c "%u:%g %a %n" /home/runner/_work/dind-probe'
runner_status=0
if docker run --rm -i --volume "$volume:/home/runner/_work" "$runner_image" </dev/null; then
  exit 1
else
  runner_status=$?
fi
test "$runner_status" -eq 2
printf 'empty-payload-entrypoint-status=%s\n' "$runner_status"
docker run --rm --volume "$volume:/home/runner/_work" --entrypoint /bin/sh "$runner_image" -ec 'id; stat -c "%u:%g %a %n" /home/runner/_work; test "$(cat /home/runner/_work/dind-probe)" = dind-ok; test -z "$(find /home/runner/_work -maxdepth 1 -name "jit.*" -print -quit)"; printf "runner-ok\\n" > /home/runner/_work/runner-probe; stat -c "%u:%g %a %n" /home/runner/_work/runner-probe; cat /home/runner/_work/dind-probe'
docker volume inspect "$volume" --format 'labels={{.Labels}}'
docker volume rm "$volume"
if docker volume inspect "$volume" > /tmp/runner48-volume-freshness-afterinspect-20261005.txt 2>&1; then
  cat /tmp/runner48-volume-freshness-afterinspect-20261005.txt
  exit 1
else
  afterinspect_status=$?
  printf 'postremove-inspect-status=%s\n' "$afterinspect_status"
  cat /tmp/runner48-volume-freshness-afterinspect-20261005.txt
  test "$afterinspect_status" -eq 1
  grep -F 'no such volume' /tmp/runner48-volume-freshness-afterinspect-20261005.txt
fi
printf 'cleanup=confirmed volume=%s absent\n' "$volume"
```

The pre-create witness was `precreate-inspect-status=1` with
`Error response from daemon: get velnor-runner48-proof-freshness-20261005: no
such volume`. The DinD-first mount copied up `/home/runner/_work` as
`1000:1000`, mode `0755`; root wrote its marker as `0:0`, mode `0644`. The
actual runner entrypoint rejected empty input with status 2. The subsequent
default `runner` user was `uid=1000`, read the DinD marker, and wrote its own
marker as `1000:1000`, mode `0644`. Volume labels were
`velnor.role=work` and `velnor.worker=runner48-proof-freshness-20261005`.
After removal, a second inspect again returned status 1 and `no such volume`.
The full command trace is `/tmp/runner48-volume-freshness-probe-20261005.log`
and the executed script is `/tmp/runner48-volume-freshness-probe-20261005.sh`.
This is a fresh-volume, normal-exit probe; it does not assert crash or forced
termination behavior.

Observed follow-up output:

```text
dind=sha256:c75f47ba6049197aa3ca0161bcaaf29eb4756b6c63f370e1e65bfec234d49395 linux/amd64
runner=sha256:28183f6f55daa486d1901415ffcf6d884930bed120ab7a70e57dced77789ec46 linux/amd64 runner /home/runner ["/usr/local/bin/velnor-runner-entrypoint"]
precreate-inspect-status=1
[]
Error response from daemon: get velnor-runner48-proof-freshness-20261005: no such volume
velnor-runner48-proof-freshness-20261005
created-volume=velnor-runner48-proof-freshness-20261005 worker=runner48-proof-freshness-20261005 role=work
uid=0(root) gid=0(root) groups=0(root)
1000:1000 755 /home/runner/_work
0:0 644 /home/runner/_work/dind-probe
empty jit
empty-payload-entrypoint-status=2
uid=1000(runner) gid=1000(runner) groups=1000(runner),999(docker)
1000:1000 755 /home/runner/_work
1000:1000 644 /home/runner/_work/runner-probe
dind-ok
labels=map[velnor.role:work velnor.worker:runner48-proof-freshness-20261005]
velnor-runner48-proof-freshness-20261005
postremove-inspect-status=1
[]
Error response from daemon: get velnor-runner48-proof-freshness-20261005: no such volume
cleanup=confirmed volume=velnor-runner48-proof-freshness-20261005 absent
```
