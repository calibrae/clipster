FROM rust:1.85-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY clipster-common clipster-common
COPY clipster-server clipster-server
COPY clipster-agent clipster-agent
COPY clipster-cli clipster-cli
COPY web web

RUN cargo build --release -p clipster-server

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/clipster-server /usr/local/bin/clipster-server

RUN mkdir -p /data/images

EXPOSE 8743
VOLUME /data

CMD ["clipster-server", "--bind", "0.0.0.0:8743"]
