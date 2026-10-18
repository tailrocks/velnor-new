# Target platform: linux/amd64. ARM64 is not the baseline.
# JIT is not a build ARG, ENV, label, or layer.
# disableUpdate is a scale-set registration invariant, not an image setting.
# Do not set RUNNER_ALLOW_RUNASROOT. The runner user is not root.
FROM ubuntu:26.04@sha256:42527649dbc4f9197a6073f426f488cbff0e9cc9f2afce32d91c383e4875634a AS gnu-tar-build

ARG DEBIAN_FRONTEND=noninteractive

RUN test "$(dpkg --print-architecture)" = amd64 \
    && test "$(uname -m)" = x86_64

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        acl \
        attr \
        autoconf \
        automake \
        autopoint \
        bison \
        build-essential \
        ca-certificates \
        debhelper \
        dh-exec \
        gettext \
        gzip \
        gpgv \
        libacl1-dev \
        libcap2-bin \
        libselinux1-dev \
        lzip \
        lzop \
        patch \
        pkg-config \
        python3 \
        texinfo \
        util-linux \
        zstd \
        xz-utils \
    && rm -rf /var/lib/apt/lists/* \
    && if id ubuntu >/dev/null 2>&1; then userdel --remove ubuntu; fi \
    && if getent group ubuntu >/dev/null; then groupdel ubuntu; fi \
    && groupadd --gid 1000 tarbuilder \
    && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash tarbuilder

WORKDIR /build
COPY build-gnu-tar.sh gnu-tar-source.lock.json gnu-tar-openat2-declaration.patch /build/
RUN --mount=type=tmpfs,target=/tmp,size=15032385536 \
    chmod 0755 /build/build-gnu-tar.sh \
    && TMPDIR=/tmp /build/build-gnu-tar.sh /out

FROM ubuntu:26.04@sha256:42527649dbc4f9197a6073f426f488cbff0e9cc9f2afce32d91c383e4875634a AS runner

ARG RUNNER_VERSION=2.338.0
ARG RUNNER_SHA256=af4b794c1bc41d73d40535e3fe092a39f9679cd8d965954c2aca25a05ca41d32
ARG DOCKER_VERSION=29.9.1
ARG DOCKER_SHA256=c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57
ARG COMPOSE_VERSION=5.6.0
ARG COMPOSE_SHA256=40343e21ca777173e69cff5dbafeb37c6f81f3b0d57d9e597f036e95eb63e76a
ARG BUILDX_VERSION=0.38.0
ARG BUILDX_SHA256=4fe4cc38adf48169132749b6ca22a990928db0118e3407584ee553723115d287
ARG NODE_VERSION=26.12.0
ARG NODE_SHA256=d9f88af72ad34f7ddf8e67289829f1698f54b6254f7d4f54b7453117ff1a3d2c
ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
        gzip \
        libacl1 \
        libselinux1 \
        python3 \
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
    && mkdir -p /home/runner/_work /home/runner/work

COPY --from=gnu-tar-build /out/usr/bin/tar /usr/bin/tar
RUN echo '035ec786cea68c6da6065e8c8d78e9b08aed5bbd168dee4e52da4a56417ba4c6  /usr/bin/tar' | sha256sum -c - \
    && /usr/bin/tar --version | grep -F 'tar (GNU tar) 1.35'

WORKDIR /home/runner

USER root
COPY runner-home.py /usr/local/libexec/velnor/runner-home.py
RUN chmod 0555 /usr/local/libexec/velnor/runner-home.py \
    && mkdir -p /opt/runner /usr/share/velnor \
    && curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /tmp/actions-runner.tar.gz \
        "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" \
    && echo "${RUNNER_SHA256}  /tmp/actions-runner.tar.gz" | sha256sum -c - \
    && /usr/bin/tar -xzf /tmp/actions-runner.tar.gz -C /opt/runner \
    && rm -f /tmp/actions-runner.tar.gz \
    && chown -R root:root /opt/runner \
    && python3 /usr/local/libexec/velnor/runner-home.py manifest \
        --root /opt/runner \
        --runner-version "${RUNNER_VERSION}" \
        --archive-url "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" \
        --archive-sha256 "${RUNNER_SHA256}" \
        --output /tmp/runner-home-manifest.before.json \
    && /opt/runner/bin/installdependencies.sh \
    && python3 /usr/local/libexec/velnor/runner-home.py manifest \
        --root /opt/runner \
        --runner-version "${RUNNER_VERSION}" \
        --archive-url "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" \
        --archive-sha256 "${RUNNER_SHA256}" \
        --output /tmp/runner-home-manifest.after.json \
    && cmp -s /tmp/runner-home-manifest.before.json /tmp/runner-home-manifest.after.json \
    && install -o root -g root -m 0444 /tmp/runner-home-manifest.before.json /usr/share/velnor/runner-home-manifest.json \
    && chown -R root:root /opt/runner \
    && test -x /opt/runner/bin/Runner.Listener

# The official dependency installer does not change the Runner seed.
RUN rm -rf /var/lib/apt/lists/*

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
        /usr/local/lib/docker/cli-plugins/docker-buildx \
    && docker --version | grep -F "Docker version ${DOCKER_VERSION}," \
    && docker compose version | grep -F "v${COMPOSE_VERSION}" \
    && docker buildx version | grep -F "v${BUILDX_VERSION}"

COPY entrypoint.sh /usr/local/bin/velnor-runner-entrypoint
COPY wait-docker-sock.sh /usr/local/bin/wait-docker-sock
COPY wait-docker-api.sh /usr/local/bin/velnor-wait-docker-api
COPY clear-dockerenv.sh /usr/local/bin/clear-dockerenv
RUN chmod 0755 /usr/local/bin/velnor-runner-entrypoint /usr/local/bin/wait-docker-sock /usr/local/bin/velnor-wait-docker-api /usr/local/bin/clear-dockerenv \
    && apt-get update \
    && apt-get install -y --no-install-recommends build-essential clang cmake git-lfs jq libclang-dev libssl-dev netcat-openbsd perl pkg-config protobuf-compiler python3 zstd \
    && cc --version \
    && pkg-config --modversion openssl \
    && cmake --version \
    && jq --version \
    && python3 --version \
    && python3 -c 'import socket,subprocess,threading; s=socket.socket(); s.bind(("127.0.0.1",0)); s.listen(1); port=s.getsockname()[1]; threading.Thread(target=s.accept, daemon=True).start(); raise SystemExit(subprocess.run(["nc","-z","-w","1","127.0.0.1",str(port)]).returncode)' \
    && test -n "$(find /usr/lib -path '/usr/lib/llvm-*/lib/libclang.so' -print -quit)" \
    && protoc --version \
    && printf 'syntax = "proto3"; message M { int32 x = 1; }\n' > /tmp/m.proto \
    && protoc -o /tmp/m.pb -I /tmp /tmp/m.proto \
    && test -s /tmp/m.pb \
    && rm -rf /var/lib/apt/lists/* \
    && git lfs version \
    && echo '035ec786cea68c6da6065e8c8d78e9b08aed5bbd168dee4e52da4a56417ba4c6  /usr/bin/tar' | sha256sum -c -

COPY --from=resource_probe resource-probe /velnor/resource-probe
RUN chmod 0555 /velnor/resource-probe

COPY tar-semantics-test.sh tar-semantics-planned-test.sh tar-absolute-test.sh tar-absolute-test-extended.sh /tmp/velnor-gnu-tar-tests/
RUN chmod 0755 /tmp/velnor-gnu-tar-tests/*.sh \
    && for test_file in /tmp/velnor-gnu-tar-tests/*.sh; do "$test_file"; done \
    && rm -rf /tmp/velnor-gnu-tar-tests

COPY runner-home.py runner-home-test.py /tmp/velnor-runner-home-tests/
RUN python3 /tmp/velnor-runner-home-tests/runner-home-test.py \
    && rm -rf /tmp/velnor-runner-home-tests

# Job steps call `node` on PATH. The runner's private action runtime is separate
# and is Node 24 in actions/runner 2.338.0.
RUN curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /tmp/node.tar.gz \
        "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.gz" \
    && echo "${NODE_SHA256}  /tmp/node.tar.gz" | sha256sum -c - \
    && /usr/bin/tar -xzf /tmp/node.tar.gz --strip-components=1 -C /usr/local \
    && rm -f /tmp/node.tar.gz \
    && test "$(node --version)" = "v${NODE_VERSION}" \
    && test "$(node -e 'console.log("js-action-ok")')" = js-action-ok

USER runner
ENTRYPOINT ["/usr/local/bin/velnor-runner-entrypoint"]
