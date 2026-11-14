FROM rust:1.99.1-slim-trixie@sha256:f6c4a35d21dbb3c89d7a8d056c193e94fc09037f2b58dd858b09c39a47dd02d7 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --locked --release --bin velnor-local

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
RUN apt-get update && apt-get install -y --no-install-recommends git && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/velnor-local /usr/local/bin/velnor-local
ENTRYPOINT ["velnor-local"]
