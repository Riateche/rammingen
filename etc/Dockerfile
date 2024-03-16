FROM rust:1.80.1-bookworm AS builder
COPY . /
RUN cargo build --release --bin rammingen-server --bin rammingen-admin

FROM debian:bookworm-20240812
RUN apt-get update && \
    apt-get install -y openssl && \
    rm -rf /var/lib/apt/lists/*
COPY --from=builder target/release/rammingen-server /sbin/
COPY --from=builder target/release/rammingen-admin /sbin/
ENV RUST_BACKTRACE=1
ENTRYPOINT ["/sbin/rammingen-server"]
