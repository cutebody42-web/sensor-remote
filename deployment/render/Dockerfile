FROM rust:1.98-bookworm

WORKDIR /src
COPY . .
RUN cargo build --release --locked -p sensor-rendezvous

ENV RUST_BACKTRACE=1
USER 65532:65532
EXPOSE 10000
CMD ["/src/target/release/sensor-rendezvous"]
