# Private DinD image

Platform: `linux/amd64`. ARM64 is not the baseline. One `dockerd` per worker.

```sh
docker build --platform linux/amd64 -t velnor-dind:29.9.1 images/dind
```

Docker Engine `29.9.1` is the latest [Moby release](https://github.com/moby/moby/releases/tag/v29.9.1),
downloaded from the [official static archive](https://download.docker.com/linux/static/stable/x86_64/docker-29.9.1.tgz).
SHA256 `c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57`
was measured from that official HTTPS artifact because Docker does not publish
a checksum sidecar for it. The archive was 86,364,663 bytes, with Last-Modified
`2026-10-16 09:29:12 GMT` and ETag `b17406f2778a6fa5fb95a7be40c32549`.

Listens only on `unix:///var/run/docker.sock` inside the worker namespace
(`/var/run` is `/run` on Ubuntu). Data root is `/var/lib/docker`. Group
`docker` is gid `999`. Storage driver is `vfs` because the containerd overlay
snapshotter returns `EINVAL` when this amd64 daemon runs under emulation.
The Dockerfile does not copy or mount a host socket and the entrypoint does
not prune.

The entrypoint starts `dockerd` on a private socket and links
`/run/docker.sock` only after that socket appears. Startup fails after a
bounded wait of about 10 seconds if the daemon does not create it. Each job's
Docker configuration selects and pulls the service images it needs. The
daemon and runner stay `linux/amd64`; the data root stays `/var/lib/docker` on
a DinD-only volume so vfs copies are not whiteouts on the container layer.

The image does not set `privileged`. The controller must:

- Run one container for one worker and grant only the privileges that
  private dockerd needs. Do not publish a TCP port.
- Mount a private named volume at `/run` so the socket is
  `/var/run/docker.sock`, and another at `/var/lib/docker`.
- Mount the runner's work volume at `/home/runner/_work`. The image seeds that
  path as uid/gid `1000:1000`, mode `0755`, before Docker copies it into a new
  empty named volume.
- Never mount the host or outer engine socket.
- Share that socket (and the runner work paths) with the matching runner
  container so both resolve `/var/run/docker.sock` to this worker only.
