FROM --platform=linux/amd64 ubuntu:26.04@sha256:42527649dbc4f9197a6073f426f488cbff0e9cc9f2afce32d91c383e4875634a

SHELL ["/bin/bash", "-e", "-u", "-o", "pipefail", "-c"]

ARG DEBIAN_FRONTEND=noninteractive

RUN test "$(dpkg --print-architecture)" = amd64 \
    && test "$(uname -m)" = x86_64

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        bash \
        ca-certificates \
        coreutils \
        curl \
        git \
        gzip \
        netcat-openbsd \
        python3 \
        tar \
        unzip \
        xz-utils \
        zstd \
    && rm -rf /var/lib/apt/lists/*

RUN curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL \
        -o /tmp/docker.tgz \
        https://download.docker.com/linux/static/stable/x86_64/docker-29.9.1.tgz \
    && printf '%s  %s\n' \
        c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57 \
        /tmp/docker.tgz \
        | sha256sum --check --status \
    && tar -xzf /tmp/docker.tgz --strip-components=1 -C /usr/local/bin docker/docker \
    && rm -f /tmp/docker.tgz \
    && chmod 0755 /usr/local/bin/docker

RUN mkdir -p /usr/local/lib/docker/cli-plugins \
    && curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL \
        -o /usr/local/lib/docker/cli-plugins/docker-compose \
        https://github.com/docker/compose/releases/download/v5.6.0/docker-compose-linux-x86_64 \
    && printf '%s  %s\n' \
        40343e21ca777173e69cff5dbafeb37c6f81f3b0d57d9e597f036e95eb63e76a \
        /usr/local/lib/docker/cli-plugins/docker-compose \
        | sha256sum --check --status \
    && curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL \
        -o /usr/local/lib/docker/cli-plugins/docker-buildx \
        https://github.com/docker/buildx/releases/download/v0.38.0/buildx-v0.38.0.linux-amd64 \
    && printf '%s  %s\n' \
        4fe4cc38adf48169132749b6ca22a990928db0118e3407584ee553723115d287 \
        /usr/local/lib/docker/cli-plugins/docker-buildx \
        | sha256sum --check --status \
    && chmod 0755 \
        /usr/local/lib/docker/cli-plugins/docker-compose \
        /usr/local/lib/docker/cli-plugins/docker-buildx

RUN curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL \
        -o /tmp/node.tar.gz \
        https://nodejs.org/dist/v26.12.0/node-v26.12.0-linux-x64.tar.gz \
    && printf '%s  %s\n' \
        d9f88af72ad34f7ddf8e67289829f1698f54b6254f7d4f54b7453117ff1a3d2c \
        /tmp/node.tar.gz \
        | sha256sum --check --status \
    && tar -xzf /tmp/node.tar.gz --strip-components=1 -C /usr/local \
    && rm -f /tmp/node.tar.gz \
    && [[ "$(node --version)" == v26.12.0 ]]

RUN [[ "$(docker --version)" == 'Docker version 29.9.1, build '* ]] \
    && [[ "$(docker compose version)" == *'v5.6.0'* ]] \
    && [[ "$(docker buildx version)" == *'v0.38.0'* ]] \
    && [[ "$(tar --version)" == 'tar (GNU tar)'* ]] \
    && [[ "$(node --version)" == v26.12.0 ]] \
    && for command in bash git curl python3 gzip tar unzip xz zstd nc; do command -v "$command" >/dev/null; done
