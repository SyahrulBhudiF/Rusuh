FROM oven/bun:1 AS frontend-builder
WORKDIR /app/frontend

COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile

COPY frontend/ ./
RUN bun run build

FROM rust:1-bookworm AS rust-builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
COPY config.example.yaml ./config.example.yaml
COPY README.md ./README.md
COPY AGENTS.md ./AGENTS.md
COPY frontend ./frontend
COPY --from=frontend-builder /app/frontend/dist ./frontend/dist

RUN cargo build --release --bin rusuh

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates python3 tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 rusuh

COPY --from=rust-builder /app/target/release/rusuh /usr/local/bin/rusuh
COPY --from=rust-builder /app/frontend/dist ./frontend/dist
COPY config.example.yaml ./config.example.yaml

RUN mkdir -p /app/data/auth /home/rusuh/.rusuh \
    && chown -R rusuh:rusuh /app /home/rusuh

ENV RUST_LOG=rusuh=info,tower_http=debug
ENV RUSUH_CONFIG=/app/config.yaml

USER rusuh
EXPOSE 8317
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["rusuh", "serve", "--config", "/app/config.yaml"]
