FROM rust:1.100.0-slim-trixie@sha256:51fdc80ccc3fb83bf14481f2cd8d0d11703eeb9f416ab47476c32a793a822307 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --locked --release --bin velnor-local

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
RUN apt-get update && apt-get install -y --no-install-recommends git && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/velnor-local /usr/local/bin/velnor-local
ENTRYPOINT ["velnor-local"]
