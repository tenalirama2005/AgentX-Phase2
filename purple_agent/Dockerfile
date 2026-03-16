# Stage 1: Build
FROM rust:1.94 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim
WORKDIR /app

# Purple Agent only needs CA certificates for Claude API calls
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/purple_agent /app/purple_agent
COPY --from=builder /app/models.toml /app/models.toml

EXPOSE 8081

# purple_agent/Dockerfile
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD bash -c 'echo > /dev/tcp/localhost/8081' || exit 1

CMD ["./purple_agent"]

