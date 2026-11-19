# Base image and toolchain package line are those of images/runner/ubuntu-26.04/Dockerfile, the
# production runner image. tests/it/plain_image.rs fails when either differs from it.
# The base is the multi-arch index (ubuntu:resolute-20261006) whose linux/amd64 manifest is the
# production pin, so Apple Silicon pulls its own platform. The test checks the platforms below.
# platforms: linux/amd64=sha256:42527649dbc4f9197a6073f426f488cbff0e9cc9f2afce32d91c383e4875634a linux/arm64=sha256:7f787d965bc0e4cd42b5f74c013515d6e24ce691c881c81c42e4db59a791e5ce
FROM ubuntu:26.04@sha256:c098726dd9b9733f3e885f8de0ef375206a311b0a317eadc40d347b1115fb944
ARG RUNNER_VERSION=2.338.0
ARG RUNNER_SHA256=628b4a7258487b80c1d3c221095a7ded349f5ae0175fd3dfab708d07428f041b
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git sudo unzip xz-utils \
    && apt-get install -y --no-install-recommends build-essential clang cmake git-lfs jq libclang-dev libssl-dev netcat-openbsd perl pkg-config protobuf-compiler python3 zstd \
    && rm -rf /var/lib/apt/lists/*
# The runner user is the production one (uid 1000, bash). The production image sets no ENV, PATH or
# mise activation, so a job starts from the same environment here; tests/it/plain_image.rs checks it.
RUN if id ubuntu >/dev/null 2>&1; then userdel --remove ubuntu; fi \
    && if getent group ubuntu >/dev/null; then groupdel ubuntu; fi \
    && groupadd --gid 1000 runner \
    && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash runner \
    && echo "runner ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/runner
WORKDIR /home/runner
RUN curl -fsSL -o runner.tar.gz "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-arm64-${RUNNER_VERSION}.tar.gz" \
    && echo "${RUNNER_SHA256}  runner.tar.gz" | sha256sum -c - \
    && tar xzf runner.tar.gz && rm runner.tar.gz \
    && ./bin/installdependencies.sh \
    && rm -rf /var/lib/apt/lists/* \
    && chown -R runner:runner /home/runner
USER runner
