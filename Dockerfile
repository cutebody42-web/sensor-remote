FROM rust:1.98.1-bookworm AS build

WORKDIR /src
COPY . .
RUN cargo build --release --locked -p sensor-rendezvous

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/sensor-rendezvous /usr/local/bin/sensor-rendezvous

ENV RUST_BACKTRACE=1
USER 65532:65532
EXPOSE 10000
CMD ["sensor-rendezvous"]
