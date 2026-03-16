FROM rust:1.94-slim
WORKDIR /app
COPY . .
RUN cargo build --release
CMD ["./target/release/ai_mcp"]
