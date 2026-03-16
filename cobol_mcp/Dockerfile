FROM rust:1.94 AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

# Runtime stage - includes GnuCOBOL
FROM rust:1.94

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    gnucobol \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/cobol_mcp .

RUN useradd -r -s /bin/false mcpuser
USER mcpuser

EXPOSE 8083

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD bash -c 'echo > /dev/tcp/localhost/8083' || exit 1

CMD ["./cobol_mcp"]
