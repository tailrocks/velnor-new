# Private DinD image

Platform: `linux/amd64`. ARM64 is not the baseline. One `dockerd` per worker.

The DinD image tag is `velnor-dind:29.9.1`.

```sh
bash images/resource-probe/build-runner-images.sh
```

Run this command from the repository root. The script does a check of the
probe executable and its source revision. It uses one named build context for
both images. See the Runner README for the pinned toolchain and probe build
steps.

The base is pinned to the Ubuntu 26.04 `linux/amd64` manifest digest in
`.velnor/runner.lock`. Docker Engine `29.9.1` is downloaded from the official
static Linux x86_64 distribution at
`https://download.docker.com/linux/static/stable/x86_64/docker-29.9.1.tgz`;
its measured SHA256 and verified `moby/moby` tag `docker-v29.9.1` at source
commit `b26fb77d3e7b01f9d21b89539c1ab428f5ca431f` are recorded in the lock.

Listens on `tcp://127.0.0.1:2375` and the private
`unix:///var/run/docker.sock` inside the worker namespace (`/var/run` is
`/run` on Ubuntu). The TCP API binds to loopback only. The Runner joins this
container's network namespace and uses the TCP endpoint for job processes;
container actions continue to use the private Unix socket. No host port is
published. Data root is `/var/lib/docker`. Group
`docker` is gid `999`. Storage driver is `vfs` because the containerd overlay
snapshotter returns `EINVAL` when this amd64 daemon runs under emulation.
The TCP endpoint uses `--tls=false` and has no authentication. Any process in
the shared worker network namespace, including the job, can fully control this
worker's Docker daemon and its containers and volumes. Loopback binding limits
reachability to that namespace; it does not isolate processes inside it. Never
publish port `2375` or expose the outer Docker socket.
The Dockerfile does not copy or mount a host socket and the entrypoint does
not prune.

The entrypoint starts `dockerd` on a private socket and fixed loopback TCP
endpoint, then links `/run/docker.sock` only after the socket appears. Startup
fails after a bounded wait of about 10 seconds if the daemon does not create
it. Each job's
Docker configuration selects and pulls the service images it needs. The
daemon and runner stay `linux/amd64`; the data root stays `/var/lib/docker` on
a DinD-only volume so vfs copies are not whiteouts on the container layer.

The image does not set `privileged`. The controller must:

- Run one container for one worker and grant only the privileges that
  private dockerd needs. Do not publish any TCP port, including `2375`.
- Mount a private named volume at `/run` so the socket is
  `/var/run/docker.sock`, and another at `/var/lib/docker`.
- Mount the same fresh Runner home volume at `/home/runner` in both containers.
- Set `NoCopy` on the Runner home volume mount.
- Set the Runner home volume root owner to UID/GID `1000:1000`.
- The Runner entrypoint copies the Runner seed into the Runner home volume
  before it reads JIT input. DinD does not copy the Runner seed into that volume.
- Mount the separate Work volume at `/home/runner/_work`.
- Set `NoCopy` on the Work volume mount.
- Both containers use the same absolute path.
- Never mount the host or outer engine socket.
- Share that socket (and the runner work paths) with the matching runner
  container so container actions resolve `/var/run/docker.sock` to this
  worker only. Set `DOCKER_HOST=tcp://localhost:2375` in the Runner container
  and join it to this exact DinD container's network namespace.

The image includes the same static `/velnor/resource-probe` file as the Runner
image. The release build does a check of the probe architecture, source
revision, and digest.
