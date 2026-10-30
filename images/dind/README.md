# Private DinD image

Platform: `linux/amd64`. ARM64 is not the baseline. One `dockerd` per worker.

```sh
docker build --platform linux/amd64 -t velnor-dind:29.9.1 images/dind
```

The base is pinned to the Ubuntu 26.04 `linux/amd64` manifest digest in
`.velnor/runner.lock`. Docker Engine `29.9.1` is downloaded from the official
static Linux x86_64 distribution at
`https://download.docker.com/linux/static/stable/x86_64/docker-29.9.1.tgz`;
its measured SHA256 and verified `moby/moby` tag `docker-v29.9.1` at source
commit `b26fb77d3e7b01f9d21b89539c1ab428f5ca431f` are recorded in the lock.

Listens on `tcp://127.0.0.1:2375` and the private
`unix:///var/run/docker.sock` inside the worker's network namespace
(`/var/run` is `/run` on Ubuntu). The TCP listener uses `--tls=false` because
only this job's runner shares the DinD network namespace. Any process able to
reach this loopback endpoint has full control of that job's daemon; processes
inside the same job are not isolated from one another. Controller credentials
and the outer engine socket never enter this namespace. Data root is
`/var/lib/docker`. Group
`docker` is gid `999`. Storage driver is `vfs` because the containerd overlay
snapshotter returns `EINVAL` when this amd64 daemon runs under emulation.
The Dockerfile does not copy or mount a host socket and the entrypoint does
not prune.

The entrypoint starts `dockerd` on both private endpoints and links
`/run/docker.sock` only after the Unix socket appears. Startup fails after a
bounded wait of about 10 seconds if the daemon does not create it. The runner
uses the loopback endpoint. Official container actions receive the Unix socket
mount. Each job's
Docker configuration selects and pulls the service images it needs. The
daemon and runner stay `linux/amd64`; the data root stays `/var/lib/docker` on
a DinD-only volume so vfs copies are not whiteouts on the container layer.

The image does not set `privileged`. The controller must:

- Run one container for one worker and grant only the privileges that
  private dockerd needs. Keep the runner in this DinD container's network
  namespace. Do not publish a TCP port or use host networking.
- Mount a private named volume at `/run` so the socket is
  `/var/run/docker.sock`, and another at `/var/lib/docker`.
- Mount the runner's work volume at `/home/runner/_work`. The image seeds that
  path as uid/gid `1000:1000`, mode `0755`, before Docker copies it into a new
  empty named volume.
- Never mount the host or outer engine socket.
- Share the Unix socket (and runner work paths) with this worker's runner.
  Keep `DOCKER_HOST=tcp://127.0.0.1:2375` on runner processes so published
  service ports and Testcontainers use their job-local loopback. A nested
  container with its own network namespace must use the mounted Unix socket
  or its own explicit endpoint; it must not inherit the runner's loopback URL.
