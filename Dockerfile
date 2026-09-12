FROM node:22-bookworm-slim AS frontend-builder

WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:1-bookworm AS backend-builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
RUN cargo build --release --locked --bin round-robin-quorum

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 app

WORKDIR /app
COPY --from=backend-builder /build/target/release/round-robin-quorum /app/bin/round-robin-quorum
COPY --from=frontend-builder /build/frontend/dist/ /app/frontend/dist/

ENV RRQ_FRONTEND_DIR=/app/frontend/dist
EXPOSE 3000

USER app

HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=5 \
    CMD curl --fail --silent --show-error "http://127.0.0.1:${PORT:-3000}/api/health" > /dev/null || exit 1

CMD ["/app/bin/round-robin-quorum"]
