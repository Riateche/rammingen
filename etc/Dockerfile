FROM rust:1.88.0-bookworm AS builder
RUN apt-get update && \
    apt-get install -y libdbus-1-dev
COPY . /
RUN cargo build --release --locked \
    --bin rammingen \
    --bin rammingen-admin \
    --bin rammingen-server

FROM debian:bookworm-20250630
RUN apt-get update && \
    apt-get install -y \
        gettext-base \
        openssl && \
    rm -rf /var/lib/apt/lists/*
COPY --from=builder \
    target/release/rammingen \
    target/release/rammingen-admin \
    target/release/rammingen-server \
    /sbin/
ENV RUST_BACKTRACE=1
ENTRYPOINT ["/sbin/rammingen-server"]
