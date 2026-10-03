# Target platform: linux/amd64. ARM64 is not the baseline.
# One dockerd per worker. Listens only on unix:///var/run/docker.sock.
# Do not COPY or mount a host socket. No TCP listener. No prune.
# This image does not set privileged. The controller supplies runtime privileges.
FROM ubuntu:26.04

ARG DOCKER_VERSION=29.8.2
ARG DOCKER_SHA256=995d1ef289677f74fd58d8d2c35727b6a4ee389c69db8638a3e42d0487aa5b0f
ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        gzip \
        iproute2 \
        iptables \
        tar \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 999 docker \
    && mkdir -p /var/lib/docker /run \
    && curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /tmp/docker.tgz \
        "https://download.docker.com/linux/static/stable/x86_64/docker-${DOCKER_VERSION}.tgz" \
    && echo "${DOCKER_SHA256}  /tmp/docker.tgz" | sha256sum -c - \
    && tar -xzf /tmp/docker.tgz --strip-components=1 -C /usr/local/bin \
    && rm -f /tmp/docker.tgz \
    && chmod 0755 \
        /usr/local/bin/containerd \
        /usr/local/bin/containerd-shim-runc-v2 \
        /usr/local/bin/ctr \
        /usr/local/bin/docker \
        /usr/local/bin/docker-init \
        /usr/local/bin/docker-proxy \
        /usr/local/bin/dockerd \
        /usr/local/bin/runc

# containerd's overlay snapshotter returns EINVAL when this amd64 dockerd
# runs under emulation. vfs does not mount overlay. linux/amd64 stays the baseline.
RUN mkdir -p /etc/docker \
    && printf '%s\n' \
        '{' \
        '  "storage-driver": "vfs",' \
        '  "features": { "containerd-snapshotter": false },' \
        '  "group": "docker",' \
        '  "data-root": "/var/lib/docker",' \
        '  "iptables": true' \
        '}' > /etc/docker/daemon.json

ENTRYPOINT ["dockerd", "--host=unix:///var/run/docker.sock"]
