# Target platform: linux/amd64. ARM64 is not the baseline.
# JIT is not a build ARG, ENV, label, or layer.
# disableUpdate is a scale-set registration invariant, not an image setting.
# Do not set RUNNER_ALLOW_RUNASROOT. The runner user is not root.
FROM ubuntu:26.04

ARG RUNNER_VERSION=2.337.0
ARG RUNNER_SHA256=70920811a4f8ad4328818682bca5c6469c1c942fab52448868071d0063816613
ARG DOCKER_VERSION=29.8.2
ARG DOCKER_SHA256=995d1ef289677f74fd58d8d2c35727b6a4ee389c69db8638a3e42d0487aa5b0f
ARG COMPOSE_VERSION=5.6.0
ARG COMPOSE_SHA256=40343e21ca777173e69cff5dbafeb37c6f81f3b0d57d9e597f036e95eb63e76a
ARG BUILDX_VERSION=0.37.2
ARG BUILDX_SHA256=982ca20490b45ed1ec8d99795974d3d874a358f75938c9c237305010e6b7e548
ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
        gzip \
        sudo \
        tar \
    && rm -rf /var/lib/apt/lists/* \
    && if id ubuntu >/dev/null 2>&1; then userdel --remove ubuntu; fi \
    && if getent group ubuntu >/dev/null; then groupdel ubuntu; fi \
    && groupadd --gid 1000 runner \
    && groupadd --gid 999 docker \
    && useradd --uid 1000 --gid 1000 --groups docker --create-home --shell /bin/bash runner \
    && printf 'runner ALL=(ALL) NOPASSWD:ALL\n' > /etc/sudoers.d/runner \
    && chmod 0440 /etc/sudoers.d/runner \
    && visudo -cf /etc/sudoers.d/runner \
    && mkdir -p /home/runner/_work \
    && chown -R runner:runner /home/runner

WORKDIR /home/runner

# GNU tar from Ubuntu 26.04 returns ENOSYS (Function not implemented) when
# extracting this archive under qemu-user. busybox tar unpacks the official
# tarball unchanged. It is not kept in the image.
USER root
RUN apt-get update \
    && apt-get install -y --no-install-recommends busybox \
    && curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /tmp/actions-runner.tar.gz \
        "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" \
    && echo "${RUNNER_SHA256}  /tmp/actions-runner.tar.gz" | sha256sum -c - \
    && busybox tar -xzf /tmp/actions-runner.tar.gz -C /home/runner \
    && rm -f /tmp/actions-runner.tar.gz \
    && chown -R runner:runner /home/runner \
    && apt-get purge -y busybox \
    && apt-get autoremove -y \
    && rm -rf /var/lib/apt/lists/*

# Official dependency installer. Does not patch Runner.Listener or os-release.
RUN /home/runner/bin/installdependencies.sh \
    && rm -rf /var/lib/apt/lists/*

RUN curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /tmp/docker.tgz \
        "https://download.docker.com/linux/static/stable/x86_64/docker-${DOCKER_VERSION}.tgz" \
    && echo "${DOCKER_SHA256}  /tmp/docker.tgz" | sha256sum -c - \
    && tar -xzf /tmp/docker.tgz --strip-components=1 -C /usr/local/bin docker/docker \
    && rm -f /tmp/docker.tgz \
    && mkdir -p /usr/local/lib/docker/cli-plugins \
    && curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /usr/local/lib/docker/cli-plugins/docker-compose \
        "https://github.com/docker/compose/releases/download/v${COMPOSE_VERSION}/docker-compose-linux-x86_64" \
    && echo "${COMPOSE_SHA256}  /usr/local/lib/docker/cli-plugins/docker-compose" | sha256sum -c - \
    && curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /usr/local/lib/docker/cli-plugins/docker-buildx \
        "https://github.com/docker/buildx/releases/download/v${BUILDX_VERSION}/buildx-v${BUILDX_VERSION}.linux-amd64" \
    && echo "${BUILDX_SHA256}  /usr/local/lib/docker/cli-plugins/docker-buildx" | sha256sum -c - \
    && chmod 0755 /usr/local/bin/docker \
        /usr/local/lib/docker/cli-plugins/docker-compose \
        /usr/local/lib/docker/cli-plugins/docker-buildx

COPY entrypoint.sh /usr/local/bin/velnor-runner-entrypoint
RUN chmod 0755 /usr/local/bin/velnor-runner-entrypoint \
    && chown -R runner:runner /home/runner

USER runner
ENTRYPOINT ["/usr/local/bin/velnor-runner-entrypoint"]
