FROM rust:1.80.1-bookworm AS builder
COPY . /
RUN cargo build --release --locked \
    --bin rammingen \
    --bin rammingen-admin \
    --bin rammingen-server

FROM debian:bookworm-20240812
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
