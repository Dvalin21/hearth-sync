# hearth-sync: multi-stage Rust build
FROM rust:1.85-slim AS builder
WORKDIR /app
COPY Cargo.toml ./
# cache deps
RUN mkdir src && echo 'fn main(){}' > src/main.rs && cargo build --release 2>/dev/null || true
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/hearth-sync /usr/local/bin/hearth-sync
ENV HEARTH_SYNC_LISTEN=0.0.0.0:8080
ENV HEARTH_SYNC_DATA_DIR=/data
ENV HEARTH_SYNC_LOG=info
EXPOSE 8080
VOLUME ["/data"]
ENTRYPOINT ["/usr/local/bin/hearth-sync"]
EOF
cat > /home/keith/host/hearth-sync/docker-compose.yml <<'EOF'
services:
  hearth-sync:
    build: .
    container_name: hearth-sync
    ports:
      - "8080:8080"
    environment:
      HEARTH_SYNC_JWT_SECRET: ${HEARTH_SYNC_JWT_SECRET:?set in .env}
      HEARTH_SYNC_LISTEN: "0.0.0.0:8080"
      HEARTH_SYNC_DATA_DIR: "/data"
      HEARTH_SYNC_MAX_UPLOAD_MB: "100"
    volumes:
      - ./data:/data
    restart: unless-stopped
EOF
echo done