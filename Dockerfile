FROM rust:1.69.0-buster AS builder
COPY . /
RUN cargo build --release --bin rammingen-server --bin rammingen-admin

FROM debian:buster-20230522
RUN apt-get update && apt-get install -y openssl
COPY --from=builder target/release/rammingen-server /sbin/
COPY --from=builder target/release/rammingen-admin /sbin/
ENV RUST_BACKTRACE=1
ENTRYPOINT ["/sbin/rammingen-server"]
