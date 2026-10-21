# hearth-sync: multi-stage Rust build
FROM rust:1.99-slim AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev g++ && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs && cargo build --release 2>/dev/null || true
COPY src ./src
COPY ml ./ml
RUN cargo build --release --features ml

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates python3 python3-pip && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/hearth-sync /usr/local/bin/hearth-sync
COPY ml /app/ml
ENV HEARTH_SYNC_LISTEN=0.0.0.0:8080
ENV HEARTH_SYNC_DATA_DIR=/data
ENV HEARTH_SYNC_LOG=info
ENV HEARTH_SYNC_MODEL_DIR=/data/models
EXPOSE 8080
VOLUME ["/data"]
ENTRYPOINT ["/usr/local/bin/hearth-sync"]
