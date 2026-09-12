FROM rust:latest AS builder
WORKDIR /app
COPY Cargo.* ./
COPY src ./src
COPY tests ./tests
COPY scripts ./scripts
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/round-robin-quorum /app/round-robin-quorum
ENV RRQ_ADDR=0.0.0.0:3000
EXPOSE 3000
CMD ["/app/round-robin-quorum"]

