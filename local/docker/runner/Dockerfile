# Base image and toolchain package line are those of images/runner/ubuntu-26.04/Dockerfile, the
# production runner image. tests/it/plain_image.rs fails when either differs from it.
FROM ubuntu:26.04@sha256:42527649dbc4f9197a6073f426f488cbff0e9cc9f2afce32d91c383e4875634a
ARG RUNNER_VERSION=2.338.0
ARG RUNNER_SHA256=628b4a7258487b80c1d3c221095a7ded349f5ae0175fd3dfab708d07428f041b
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git sudo unzip xz-utils \
    && apt-get install -y --no-install-recommends build-essential clang cmake git-lfs jq libclang-dev libssl-dev netcat-openbsd perl pkg-config protobuf-compiler python3 zstd \
    && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 1001 runner && echo "runner ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/runner
WORKDIR /home/runner
RUN curl -fsSL -o runner.tar.gz "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-arm64-${RUNNER_VERSION}.tar.gz" \
    && echo "${RUNNER_SHA256}  runner.tar.gz" | sha256sum -c - \
    && tar xzf runner.tar.gz && rm runner.tar.gz \
    && ./bin/installdependencies.sh \
    && rm -rf /var/lib/apt/lists/* \
    && chown -R runner:runner /home/runner
USER runner
