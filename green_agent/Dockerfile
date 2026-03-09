# --- Stage 1: Builder ---
FROM rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

# --- Stage 2: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

# Only needs CA certificates and OpenSSL for HTTPS calls
RUN apt-get update && \
    apt-get install -y \
    ca-certificates \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/green_agent .
RUN chmod +x ./green_agent

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD bash -c 'echo > /dev/tcp/localhost/8080' || exit 1

CMD ["./green_agent"]