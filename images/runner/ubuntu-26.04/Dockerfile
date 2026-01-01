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
ARG NODE_VERSION=24.17.0
ARG NODE_SHA256=e0472427aa791ad80bdc426ff7cc73cdd28ed0f616d1ff9689a23a7f47f1265f
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
    && mkdir -p /home/runner/_work /home/runner/work \
    && chown -R runner:runner /home/runner

WORKDIR /home/runner

# GNU tar from Ubuntu 26.04 calls openat2. qemu-user returns ENOSYS, and
# glibc does not fall back, so extract fails with "Function not implemented".
# busybox tar unpacks the official tarball and stays the tar the runner execs.
USER root
RUN apt-get update \
    && apt-get install -y --no-install-recommends busybox \
    && curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /tmp/actions-runner.tar.gz \
        "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" \
    && echo "${RUNNER_SHA256}  /tmp/actions-runner.tar.gz" | sha256sum -c - \
    && busybox tar -xzf /tmp/actions-runner.tar.gz -C /home/runner \
    && rm -f /tmp/actions-runner.tar.gz \
    && chown -R runner:runner /home/runner \
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
COPY wait-docker-sock.sh /usr/local/bin/wait-docker-sock
COPY wait-docker-api.sh /usr/local/bin/velnor-wait-docker-api
COPY clear-dockerenv.sh /usr/local/bin/clear-dockerenv
COPY tar-shim.sh /usr/local/bin/velnor-tar
COPY tar-absolute.sh /usr/local/bin/tar-absolute.sh
COPY tar-extract.sh /usr/local/bin/tar-extract.sh
COPY tar-extract-plan.sh /usr/local/bin/tar-extract-plan.sh
COPY tar-member.pl /usr/local/bin/tar-member.pl
COPY tar-dir-meta.pl /usr/local/bin/tar-dir-meta.pl
COPY tar-member-stream.pl /usr/local/bin/tar-member-stream.pl
COPY tar-member-rewrite.pl /usr/local/bin/tar-member-rewrite.pl
COPY tar-pax.pl /usr/local/bin/velnor-tar-pax
# zstd and git-lfs are installed before the tar divert. dpkg needs GNU tar to unpack.
# After the divert, /usr/bin/tar is the BusyBox shim, not GNU tar.
# netcat-openbsd provides `nc -z`. Cancel-service probes the service port with it.
RUN chmod 0755 /usr/local/bin/velnor-runner-entrypoint /usr/local/bin/wait-docker-sock /usr/local/bin/velnor-wait-docker-api /usr/local/bin/clear-dockerenv /usr/local/bin/velnor-tar /usr/local/bin/velnor-tar-pax /usr/local/bin/tar-member.pl \
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
    && chown -R runner:runner /home/runner \
    && dpkg-divert --local --rename --divert /usr/bin/tar.gnu --add /usr/bin/tar \
    && ln -s /usr/local/bin/velnor-tar /usr/bin/tar \
    && mkdir -p /tmp/tar-src/dir \
    && echo ok > /tmp/tar-src/dir/file \
    && busybox tar -czf /tmp/tar-probe.tgz -C /tmp/tar-src dir \
    && mkdir -p /tmp/tar-out \
    && tar -xzf /tmp/tar-probe.tgz -C /tmp/tar-out \
    && test "$(cat /tmp/tar-out/dir/file)" = ok \
    && mkdir -p /tmp/tar-old \
    && tar xz -C /tmp/tar-old -f /tmp/tar-probe.tgz \
    && test "$(cat /tmp/tar-old/dir/file)" = ok \
    && echo cfz-ok > /tmp/tar-src/cfz.txt \
    && tar cfz /tmp/cfz.tgz -C /tmp/tar-src cfz.txt \
    && mkdir -p /tmp/cfz-out \
    && tar xfz /tmp/cfz.tgz -C /tmp/cfz-out \
    && test "$(cat /tmp/cfz-out/cfz.txt)" = cfz-ok \
    && tar --zstd --version \
    && echo cache-ok > /tmp/tar-src/g4-cache.txt \
    && printf '%s\n' g4-cache.txt > /tmp/tar-manifest \
    && tar -cf /tmp/cache.tgz --exclude cache.tgz -P -C /tmp/tar-src --files-from /tmp/tar-manifest -z \
    && mkdir -p /tmp/tar-cache-out \
    && tar -xf /tmp/cache.tgz -P -C /tmp/tar-cache-out -z \
    && test "$(cat /tmp/tar-cache-out/g4-cache.txt)" = cache-ok \
    && tar -cf /tmp/cache.tzst --exclude cache.tzst -P -C /tmp/tar-src --files-from /tmp/tar-manifest --use-compress-program zstdmt \
    && mkdir -p /tmp/tar-zstd-out \
    && tar -xf /tmp/cache.tzst -P -C /tmp/tar-zstd-out --use-compress-program unzstd \
    && test "$(cat /tmp/tar-zstd-out/g4-cache.txt)" = cache-ok \
    && perl -e 'my $limit = 2097152; my $dir = "/tmp/argv-src"; mkdir $dir or die $!; open my $m, ">", "/tmp/argv-manifest" or die $!; my $leaf = "a" x 200; my $bytes = 0; my $n = 0; while ($bytes <= $limit) { $n++; my $name = sprintf("%s-%05d", $leaf, $n); open my $f, ">", "$dir/$name" or die $!; print $f "ok"; close $f; print $m "$name\n"; $bytes += length($name) + 1; } open my $s, ">", "$dir/short-ok" or die $!; print $s "ok"; close $s; print $m "short-ok\n"; $n++; open my $c, ">", "/tmp/argv-count" or die $!; print $c $n; close $c;' \
    && tar -cf /tmp/argv.tar -C /tmp/argv-src --files-from /tmp/argv-manifest \
    && test "$(tar.gnu -tf /tmp/argv.tar | wc -l | tr -d " ")" = "$(cat /tmp/argv-count)" \
    && mkdir -p /tmp/argv-out \
    && tar -xf /tmp/argv.tar -C /tmp/argv-out \
    && test "$(cat /tmp/argv-out/short-ok)" = ok \
    && printf "%s\n" short-ok > /tmp/argv-one \
    && mkdir -p /tmp/argv-one-out \
    && tar -xf /tmp/argv.tar -C /tmp/argv-one-out --files-from /tmp/argv-one \
    && test "$(cat /tmp/argv-one-out/short-ok)" = ok \
    && test "$(find /tmp/argv-one-out -type f | wc -l | tr -d " ")" = 1 \
    && long="$(printf 'n%.0s' $(seq 1 90))" \
    && mkdir -p "/tmp/p-home/runner/_work/repo/repo" \
    && mkdir -p "/tmp/p-home/runner/.cache/mbx/${long}/sub" \
    && echo mbx-ok > /tmp/p-home/runner/.cache/mbx/marker \
    && echo long-ok > "/tmp/p-home/runner/.cache/mbx/${long}/sub/file" \
    && echo stay > /tmp/p-home/runner/_work/repo/repo/keep.txt \
    && printf '%s\n' ../../../.cache/mbx/marker "../../../.cache/mbx/${long}/sub/file" keep.txt > /tmp/p-manifest \
    && tar -cf /tmp/p-abs.tar -P -C /tmp/p-home/runner/_work/repo/repo --files-from /tmp/p-manifest \
    && tar.gnu -tf /tmp/p-abs.tar | grep -qx '../../../.cache/mbx/marker' \
    && tar.gnu -tf /tmp/p-abs.tar | grep -qx "../../../.cache/mbx/${long}/sub/file" \
    && tar -cf /tmp/p.tzst -P -C /tmp/p-home/runner/_work/repo/repo --files-from /tmp/p-manifest --use-compress-program zstdmt \
    && echo abs-ok > /tmp/abs-marker \
    && tar -cf /tmp/abs.tar -P /tmp/abs-marker \
    && rm -rf /tmp/p-home/runner/.cache /tmp/abs-marker \
    && rm -f /tmp/p-home/runner/_work/repo/repo/keep.txt \
    && tar -xf /tmp/p-abs.tar -P -C /tmp/p-home/runner/_work/repo/repo \
    && test "$(cat /tmp/p-home/runner/.cache/mbx/marker)" = mbx-ok \
    && test "$(cat "/tmp/p-home/runner/.cache/mbx/${long}/sub/file")" = long-ok \
    && test "$(cat /tmp/p-home/runner/_work/repo/repo/keep.txt)" = stay \
    && test ! -e /tmp/p-home/runner/_work/repo/repo/.cache \
    && rm -rf /tmp/p-home/runner/.cache \
    && rm -f /tmp/p-home/runner/_work/repo/repo/keep.txt \
    && if tar -xf /tmp/p-abs.tar -C /tmp/p-home/runner/_work/repo/repo; then exit 1; fi \
    && test ! -e /tmp/p-home/runner/.cache \
    && test ! -e /tmp/p-home/runner/_work/repo/repo/.cache \
    && test ! -e /tmp/p-home/runner/_work/repo/repo/keep.txt \
    && mkdir -p /tmp/abs-out \
    && tar -xf /tmp/abs.tar -P -C /tmp/abs-out \
    && test "$(cat /tmp/abs-marker)" = abs-ok \
    && test ! -e /tmp/abs-out/tmp/abs-marker \
    && rm -rf /tmp/p-home/runner/.cache /tmp/p-home/runner/_work/repo/repo/.cache \
    && tar -xf /tmp/p.tzst -P -C /tmp/p-home/runner/_work/repo/repo --use-compress-program unzstd \
    && test "$(cat /tmp/p-home/runner/.cache/mbx/marker)" = mbx-ok \
    && rm -rf /tmp/tar-src /tmp/tar-out /tmp/tar-old /tmp/tar-probe.tgz /tmp/tar-manifest /tmp/cache.tgz /tmp/cache.tzst /tmp/tar-cache-out /tmp/tar-zstd-out /tmp/p-home /tmp/p-manifest /tmp/p-abs.tar /tmp/p.tzst /tmp/abs-marker /tmp/abs.tar /tmp/abs-out /tmp/cfz.tgz /tmp/cfz-out /tmp/m.proto /tmp/m.pb /tmp/argv-src /tmp/argv-manifest /tmp/argv-count /tmp/argv.tar /tmp/argv-out /tmp/argv-one /tmp/argv-one-out

# Job steps call `node` on PATH. Action JavaScript uses the runner's own runtime.
# /usr/bin/tar is the job shim after the divert. Listing that trusted vendor
# archive goes through GNU tar and does not finish under qemu-user. BusyBox
# unpacks it. Job archives still enter the shim.
RUN curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /tmp/node.tar.gz \
        "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.gz" \
    && echo "${NODE_SHA256}  /tmp/node.tar.gz" | sha256sum -c - \
    && busybox tar -xzf /tmp/node.tar.gz --strip-components=1 -C /usr/local \
    && rm -f /tmp/node.tar.gz \
    && node --version \
    && test "$(node -e 'console.log("js-action-ok")')" = js-action-ok

USER runner
ENTRYPOINT ["/usr/local/bin/velnor-runner-entrypoint"]
